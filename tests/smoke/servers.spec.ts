/**
 * F44, ADR-0105 — the Server mode: the address book beside the rail, one tab per
 * SSH session in the top bar, and the host key as a decision only the human
 * makes. The SSH engine itself is the Rust suite's (a real sshd on loopback);
 * this lane holds the surface's rules.
 */
import type { Server, ServerBook } from '@factorai/types';
import { expect, test } from '@playwright/test';
import { installMockBridge } from './fixtures';

function server(over: Partial<Server>): Server {
	return {
		id: 's1',
		name: 'web01',
		host: 'web01.example.com',
		port: 22,
		username: 'deploy',
		startDir: '/var/www',
		folderId: 'f1',
		environment: 'dev',
		agentPolicy: 'free',
		auth: 'agent',
		keyPath: null,
		hasPassword: false,
		hasPrivateKey: false,
		hasPassphrase: false,
		connected: false,
		...over,
	};
}

const book: ServerBook = {
	folders: [{ id: 'f1', name: 'Clients' }],
	servers: [
		server({}),
		server({
			id: 's2',
			name: 'db01',
			host: 'db01.example.com',
			environment: 'prod',
			agentPolicy: 'confirm',
		}),
	],
};

test.describe('servers', () => {
	test('@smoke the Servers mode lists the address book and marks production', async ({ page }) => {
		await installMockBridge(page, { serverBook: book });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		const list = page.getByTestId('server-book');
		await expect(list.getByText('Clients')).toBeVisible();
		await expect(page.getByTestId('server-row-web01')).toBeVisible();
		await expect(page.getByTestId('server-row-db01').getByTestId('prod-badge')).toBeVisible();
		await expect(page.getByTestId('server-row-web01').getByTestId('prod-badge')).toHaveCount(0);
	});

	test('@smoke each open is its own SSH session tab, numbered per server', async ({ page }) => {
		await installMockBridge(page, { serverBook: book });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		await page.getByTestId('server-row-web01').dblclick();
		const tabs = page.getByTestId('server-tab');
		await expect(tabs).toHaveCount(2);
		await expect(tabs.nth(0)).toContainText('web01 · 1');
		await expect(tabs.nth(1)).toContainText('web01 · 2');
		await expect(page.getByTestId('server-terminal')).toBeVisible();
		await tabs.nth(1).getByRole('button', { name: 'Close web01' }).click();
		await expect(tabs).toHaveCount(1);
		await expect(tabs.nth(0)).toContainText('web01');
	});

	test('@smoke an unknown host key waits for the human, and is sent only once trusted', async ({
		page,
	}) => {
		await installMockBridge(page, { serverBook: book, serverHostKey: 'SHA256:abc123' });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		const dialog = page.getByTestId('host-key-dialog');
		await expect(dialog).toContainText('SHA256:abc123');
		await dialog.getByTestId('trust-host-key').click();
		await expect(dialog).toHaveCount(0);
		const calls = await page.evaluate(() =>
			(window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === 'server_open_shell'),
		);
		expect(calls.map((c) => (c.args as { acceptFingerprint?: string }).acceptFingerprint)).toEqual([
			undefined,
			'SHA256:abc123',
		]);
	});

	test('@smoke the server form never sends an untouched secret', async ({ page }) => {
		await installMockBridge(page, { serverBook: book });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('new-server').click();
		const d = page.getByTestId('server-dialog');
		await d.getByLabel('Name').fill('staging');
		await d.getByPlaceholder('web01.example.com').fill('staging.example.com');
		await d.getByLabel('User').fill('ops');
		await d.getByTestId('server-save').click();
		await expect(d).toHaveCount(0);
		const saved = await page.evaluate(() =>
			(window.__FACTORAI_TEST_CALLS__ ?? []).find((c) => c.name === 'save_server'),
		);
		const input = (saved?.args as { input: Record<string, unknown> }).input;
		expect(input).toMatchObject({ name: 'staging', host: 'staging.example.com', username: 'ops' });
		expect(input).not.toHaveProperty('password');
		expect(input).not.toHaveProperty('privateKey');
	});
});
