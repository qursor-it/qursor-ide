/**
 * ADR-0117 — every mode's sidebar opens the same way: a filter with the mode's
 * primary `+`, then "In progress" while something is happening, then the mode's
 * own sections.
 */
import { expect, test } from '@playwright/test';
import {
	FOO_ID,
	fixtureOneProjectOneSession,
	fixtureWithGraph,
	installMockBridge,
	type TestFixture,
} from './fixtures';

type ServerBook = NonNullable<TestFixture['serverBook']>;
type Server = ServerBook['servers'][number];

const server = (over: Partial<Server>): Server => ({
	id: 's1',
	name: 'web01',
	host: 'web01.example.com',
	port: 22,
	username: 'deploy',
	startDir: '/var/www',
	folderId: null,
	environment: 'dev',
	agentPolicy: 'free',
	auth: 'agent',
	keyPath: null,
	persistent: false,
	hasPassword: false,
	hasPrivateKey: false,
	hasPassphrase: false,
	connected: false,
	source: { kind: 'local' },
	...over,
});

function gitFixture(conflicted: string[]): TestFixture {
	const base = fixtureWithGraph();
	const root = base.projects?.[0]?.realPath ?? '';
	const branch = (name: string, current: boolean) => ({
		name,
		sha: 'a'.repeat(40),
		upstream: null,
		ahead: 0,
		behind: 0,
		current,
	});
	return {
		...base,
		gitRefs: {
			[root]: {
				head: 'main',
				headSha: 'a'.repeat(40),
				local: [branch('main', true), branch('side', false)],
				remotes: [],
				tags: [],
				stashes: [],
			},
		},
		gitRepoStates: {
			[root]: { op: conflicted.length ? 'merge' : 'none', conflicted, theirs: null },
		},
	};
}

test.describe('sidebar: In progress and the filter row', () => {
	test('@smoke a session waiting for you is listed under In progress, and opens', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await expect(page.getByTestId('sidebar-live')).toHaveCount(0);
		await expect(page.getByTestId('sidebar-new-session')).toBeDisabled();

		await page.locator('aside nav').getByText('foo').click();
		await page.getByText('Refactor the auth middleware').click();
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await page.evaluate(() =>
			window.__FACTORAI_EMIT__?.('terminal:status', {
				id: 'mock-terminal-id',
				status: 'waiting_input',
				lastActivity: Date.now(),
			}),
		);
		const row = page.getByTestId('live-session');
		await expect(row).toContainText('Refactor the auth middleware');
		await expect(row).toContainText('foo');
		await expect(page.getByTestId('sidebar-new-session')).toBeEnabled();
	});

	test('@smoke Git lists conflicts in progress and filters branches', async ({ page }) => {
		await installMockBridge(
			page,
			gitFixture(['/home/alice/code/foo/a.ts', '/home/alice/code/foo/b.ts']),
		);
		await page.goto(`/#/projects/${FOO_ID}`);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await expect(page.getByTestId('live-git-conflicts')).toContainText('2');
		await expect(page.getByTestId('live-git-op')).toHaveText('Merge in progress');

		await expect(page.getByTestId('git-branch')).toHaveCount(2);
		await page.getByTestId('git-sidebar').getByRole('searchbox').fill('sid');
		await expect(page.getByTestId('git-branch')).toHaveCount(1);
		await expect(page.locator('[data-branch="side"]')).toBeVisible();
	});

	test('@smoke a quiet repository has no In progress section', async ({ page }) => {
		await installMockBridge(page, gitFixture([]));
		await page.goto(`/#/projects/${FOO_ID}`);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await expect(page.getByTestId('git-branch')).toHaveCount(2);
		await expect(page.getByTestId('sidebar-live')).toHaveCount(0);
	});

	test('@smoke a server with SSH sessions open is in progress; the filter narrows the book', async ({
		page,
	}) => {
		await installMockBridge(page, {
			serverBook: {
				folders: [],
				servers: [server({}), server({ id: 's2', name: 'db01', host: 'db01.example.com' })],
				problems: [],
			},
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		await expect(page.getByTestId('live-server')).toContainText('web01 · 1 SSH session');

		await page.getByTestId('sidebar').getByRole('searchbox').fill('db');
		await expect(page.getByTestId('server-row-web01')).toHaveCount(0);
		await expect(page.getByTestId('server-row-db01')).toBeVisible();
	});
});
