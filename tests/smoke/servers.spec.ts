/**
 * F44, ADR-0105 — the Server mode: the address book beside the rail, one tab per
 * SSH session in the top bar, and the host key as a decision only the human
 * makes. The SSH engine itself is the Rust suite's (a real sshd on loopback);
 * this lane holds the surface's rules.
 */
import { expect, test } from '@playwright/test';
import { installMockBridge, type TestFixture } from './fixtures';

type ServerBook = NonNullable<TestFixture['serverBook']>;
type Server = ServerBook['servers'][number];

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

	test('@smoke an agent session can share the SSH terminal or open its own tab', async ({
		page,
	}) => {
		await installMockBridge(page, {
			serverBook: book,
			claudeCli: { installed: true, binaryPath: '/usr/local/bin/claude', version: '2.1.0' },
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		await expect(page.getByTestId('server-terminal')).toBeVisible();

		await page.getByTestId('agent-menu').click();
		await page.getByTestId('agent-claude-shared').click();
		await expect(page.getByTestId('agent-pane')).toBeVisible();
		await expect(page.getByTestId('tab-agent')).toHaveCount(1);
		const calls = async (name: string) =>
			page.evaluate(
				(n) => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n),
				name,
			);
		await expect.poll(async () => (await calls('agent_link_terminal')).length).toBe(1);
		const [link] = await calls('agent_link_terminal');
		expect(link.args).toMatchObject({ mode: 'shared' });

		// Its own SSH session: a second tab, the agent's, not a tab for the agent.
		await page.getByTestId('agent-menu').click();
		await page.getByTestId('agent-detach').click();
		await expect(page.getByTestId('agent-pane')).toHaveCount(0);
		await page.getByTestId('agent-menu').click();
		await page.getByTestId('agent-claude-own').click();
		const tabs = page.getByTestId('server-tab');
		await expect(tabs).toHaveCount(2);
		await expect(tabs.nth(1).getByTestId('tab-agent')).toContainText("Claude Code's");
		await expect.poll(async () => (await calls('agent_link_terminal')).length).toBe(2);
		const own = (await calls('agent_link_terminal'))[1];
		expect(own.args).toMatchObject({ mode: 'own' });
	});

	test('@smoke an agent command on a confirm server waits for the human in a banner', async ({
		page,
	}) => {
		await installMockBridge(page, {
			serverBook: book,
			claudeCli: { installed: true, binaryPath: '/usr/local/bin/claude', version: '2.1.0' },
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-db01').dblclick();
		await page.getByTestId('agent-menu').click();
		await page.getByTestId('agent-claude-shared').click();
		const linkCall = async () =>
			page.evaluate(() =>
				(window.__FACTORAI_TEST_CALLS__ ?? []).find((c) => c.name === 'agent_link_terminal'),
			);
		await expect.poll(async () => !!(await linkCall())).toBe(true);
		const terminalId = ((await linkCall())?.args as { terminalId: string }).terminalId;
		await page.evaluate((tid) => {
			window.__FACTORAI_EMIT__?.('agent-terminal:confirm', {
				id: 'c1',
				serverId: 's2',
				serverName: 'db01',
				terminalId: tid,
				sessionId: 'x',
				command: 'sudo journalctl --vacuum-time=14d',
				keys: false,
				production: true,
			});
		}, terminalId);
		const banner = page.getByTestId('confirm-banner');
		await expect(banner).toContainText('sudo journalctl --vacuum-time=14d');
		await expect(banner).toContainText('production');
		await expect(page.getByTestId('mode-servers-count')).toHaveText('1');
		await banner.getByTestId('confirm-run').click();
		await expect(banner).toHaveCount(0);
		const answer = await page.evaluate(() =>
			(window.__FACTORAI_TEST_CALLS__ ?? []).find((c) => c.name === 'agent_terminal_answer'),
		);
		expect(answer?.args).toMatchObject({ id: 'c1', run: true });
	});

	test('@smoke the Files panel browses the server and asks before deleting', async ({ page }) => {
		await installMockBridge(page, {
			serverBook: book,
			savePick: '/home/me/Downloads/app.conf',
			remoteDirs: {
				'/var/www': {
					path: '/var/www',
					entries: [
						{ name: 'shop', isDir: true, isSymlink: false, size: 0, mtime: null },
						{ name: 'app.conf', isDir: false, isSymlink: false, size: 2048, mtime: null },
					],
					truncated: false,
				},
			},
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		await page.getByTestId('toggle-files').click();
		const panel = page.getByTestId('sftp-panel');
		await expect(panel.getByTestId('sftp-shop')).toBeVisible();
		await expect(panel.getByTestId('sftp-app.conf')).toContainText('2.0 KB');

		await panel.getByTestId('sftp-app.conf').dblclick();
		const calls = async (name: string) =>
			page.evaluate(
				(n) => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n),
				name,
			);
		await expect.poll(async () => (await calls('sftp_download')).length).toBe(1);
		expect((await calls('sftp_download'))[0].args).toMatchObject({
			remote: '/var/www/app.conf',
			local: '/home/me/Downloads/app.conf',
		});

		await panel.getByTestId('sftp-app.conf').hover();
		await panel.getByRole('button', { name: 'app.conf actions', exact: true }).click();
		await page.getByRole('menuitem', { name: 'Delete…' }).click();
		expect(await calls('sftp_remove')).toHaveLength(0);
		await page.getByRole('button', { name: 'Delete', exact: true }).click();
		await expect.poll(async () => (await calls('sftp_remove')).length).toBe(1);
		expect((await calls('sftp_remove'))[0].args).toMatchObject({
			path: '/var/www/app.conf',
			isDir: false,
		});
	});
});
