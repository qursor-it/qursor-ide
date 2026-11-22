/**
 * F45, ADR-0107 — the Server mode: the address book beside the rail, one tab per
 * SSH session in the top bar, and the host key as a decision only the human
 * makes. The SSH engine itself is the Rust suite's (a real sshd on loopback);
 * this lane holds the surface's rules.
 */
import { expect, test } from '@playwright/test';
import {
	ALPHA_ID,
	fixtureTwoProjectsManySessions,
	installMockBridge,
	type TestFixture,
	ZULU_ID,
} from './fixtures';

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
		persistent: false,
		hasPassword: false,
		hasPrivateKey: false,
		hasPassphrase: false,
		connected: false,
		source: { kind: 'local' },
		...over,
	};
}

const book: ServerBook = {
	folders: [{ id: 'f1', name: 'Clients', projectId: null }],
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
	problems: [],
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
		expect(input.home).toBe('');
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
				editable: true,
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

	test('@smoke a tunnel is a switch, and a new one is saved for its server', async ({ page }) => {
		await installMockBridge(page, {
			serverBook: book,
			tunnels: [
				{
					id: 't1',
					serverId: 's1',
					label: 'Postgres',
					localPort: 5433,
					destHost: 'localhost',
					destPort: 5432,
					running: false,
				},
			],
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		await page.getByTestId('tunnels-menu').click();
		await expect(page.getByTestId('tunnel-row')).toContainText('localhost:5433 → localhost:5432');
		await page.getByRole('switch', { name: /localhost:5433/ }).click();
		const calls = async (name: string) =>
			page.evaluate(
				(n) => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n),
				name,
			);
		await expect.poll(async () => (await calls('tunnel_start')).length).toBe(1);
		expect((await calls('tunnel_start'))[0].args).toMatchObject({ id: 't1' });

		await page.keyboard.press('Escape');
		await page.getByTestId('tunnels-menu').click();
		await page.getByRole('menuitem', { name: 'Add tunnel…' }).click();
		const d = page.getByTestId('tunnel-dialog');
		await d.getByLabel('Local port').fill('8081');
		await d.getByLabel('Destination port').fill('80');
		await d.getByTestId('tunnel-save').click();
		await expect.poll(async () => (await calls('save_tunnel')).length).toBe(1);
		expect((await calls('save_tunnel'))[0].args).toMatchObject({
			tunnel: { serverId: 's1', localPort: 8081, destHost: 'localhost', destPort: 80 },
		});
	});

	test('@smoke closing a persistent tab ends its tmux session; an ordinary tab does not ask', async ({
		page,
	}) => {
		const persistentBook: ServerBook = {
			folders: book.folders,
			servers: [server({ persistent: true }), book.servers[1]],
			problems: [],
		};
		await installMockBridge(page, { serverBook: persistentBook });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('server-row-web01').dblclick();
		await page.getByTestId('server-row-db01').dblclick();
		const tabs = page.getByTestId('server-tab');
		await expect(tabs).toHaveCount(2);
		await tabs.nth(1).getByRole('button', { name: 'Close db01' }).click();
		await tabs.nth(0).getByRole('button', { name: 'Close web01' }).click();
		await expect(tabs).toHaveCount(0);
		const ends = await page.evaluate(() =>
			(window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === 'server_end_tab'),
		);
		expect(ends).toHaveLength(1);
		expect(ends[0].args).toMatchObject({ serverId: 's1' });
	});

	// Two projects: zulu holds server files, alpha holds none.
	const projects = fixtureTwoProjectsManySessions();
	const inZulu = (over: Partial<Server>) =>
		server({
			folderId: null,
			source: {
				kind: 'project',
				projectId: ZULU_ID,
				relPath: `servers/${over.name}.server`,
				sha256: `sha-${over.name}`,
			},
			...over,
		});
	const calls = (page: import('@playwright/test').Page, name: string) =>
		page.evaluate((n) => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n), name);

	test('@smoke project servers appear under their project', async ({ page }) => {
		await installMockBridge(page, {
			...projects,
			serverBook: {
				folders: [],
				servers: [server({ folderId: null }), inZulu({ id: 's9', name: 'shared01' })],
				problems: [],
			},
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		const sections = page.getByTestId('server-section');
		await expect(sections).toHaveCount(2);
		await expect(sections.nth(0)).toContainText('This computer');
		await expect(sections.nth(1)).toContainText('zulu');
		await expect(sections.nth(1).getByTestId('server-row-shared01')).toBeVisible();
		await expect(sections.nth(0).getByTestId('server-row-web01')).toBeVisible();
	});

	test('@smoke a project with no server files is not listed', async ({ page }) => {
		await installMockBridge(page, {
			...projects,
			serverBook: { folders: [], servers: [inZulu({ id: 's9', name: 'shared01' })], problems: [] },
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await expect(page.getByTestId('server-section').filter({ hasText: 'zulu' })).toHaveCount(1);
		await expect(page.getByTestId('server-section').filter({ hasText: 'alpha' })).toHaveCount(0);
	});

	test('@smoke a duplicate offers to become a separate server', async ({ page }) => {
		await installMockBridge(page, {
			...projects,
			serverBook: {
				folders: [],
				servers: [],
				problems: [
					{
						projectId: ZULU_ID,
						relPath: 'servers/copy.server',
						kind: 'duplicate',
						message: 'same id',
					},
				],
			},
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		const row = page.getByTestId('server-problem-copy.server');
		await expect(row).toHaveAttribute('title', 'same id');
		await row.getByText('copy.server').dblclick();
		await row.getByRole('button', { name: 'Make it a separate server' }).click();
		await expect.poll(async () => (await calls(page, 'fix_server_file')).length).toBe(1);
		expect((await calls(page, 'fix_server_file'))[0].args).toEqual({
			projectId: ZULU_ID,
			relPath: 'servers/copy.server',
		});
		expect(await calls(page, 'server_open_shell')).toHaveLength(0);
	});

	test('@smoke a broken file shows its error and has no action', async ({ page }) => {
		await installMockBridge(page, {
			...projects,
			serverBook: {
				folders: [],
				servers: [],
				problems: [
					{
						projectId: ZULU_ID,
						relPath: 'servers/bad.server',
						kind: 'broken',
						message: 'bad toml',
					},
				],
			},
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		const row = page.getByTestId('server-problem-bad.server');
		await expect(row).toHaveAttribute('title', 'bad toml');
		await expect(row.getByRole('button')).toHaveCount(0);
	});

	test('@smoke save in a project sends home', async ({ page }) => {
		await installMockBridge(page, { ...projects, serverBook: book });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByTestId('new-server').click();
		const d = page.getByTestId('server-dialog');
		await d.getByTestId('server-home').click();
		await page.getByRole('option', { name: 'alpha' }).click();
		await d.getByLabel('Name').fill('shared');
		await d.getByPlaceholder('web01.example.com').fill('h.example.com');
		await d.getByLabel('User').fill('ops');
		await d.getByTestId('server-save').click();
		await expect(d).toHaveCount(0);
		const input = ((await calls(page, 'save_server'))[0].args as { input: { home: unknown } })
			.input;
		expect(input.home).toBe(ALPHA_ID);
	});

	test('@smoke editing a project server sends the file it was read from', async ({ page }) => {
		await installMockBridge(page, {
			...projects,
			serverBook: { folders: [], servers: [inZulu({ id: 's9', name: 'shared01' })], problems: [] },
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		const row = page.getByTestId('server-row-shared01');
		await row.hover();
		await row.getByRole('button', { name: 'shared01 actions' }).click();
		await page.getByRole('menuitem', { name: 'Edit…' }).click();
		const d = page.getByTestId('server-dialog');
		await d.getByLabel('Name').fill('renamed');
		await d.getByTestId('server-save').click();
		await expect(d).toHaveCount(0);
		const input = ((await calls(page, 'save_server'))[0].args as { input: { baseSha: unknown } })
			.input;
		expect(input.baseSha).toBe('sha-shared01');
	});

	test('@smoke a folder can be made inside a project section', async ({ page }) => {
		await installMockBridge(page, {
			...projects,
			serverBook: { folders: [], servers: [inZulu({ id: 's9', name: 'shared01' })], problems: [] },
		});
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await page.getByRole('button', { name: 'New folder in zulu' }).click();
		await page.getByPlaceholder('Folder name').fill('prod');
		await page.keyboard.press('Enter');
		await expect.poll(async () => (await calls(page, 'create_server_folder')).length).toBe(1);
		expect((await calls(page, 'create_server_folder'))[0].args).toEqual({
			name: 'prod',
			projectId: ZULU_ID,
		});
	});

	test('@smoke after a restart the tabs come back disconnected, and log in only on Reconnect', async ({
		page,
	}) => {
		await page.addInitScript(() => {
			const tab = { key: 'ssh:t1', serverId: 's1', terminalId: null, dead: false, agent: null };
			localStorage.setItem(
				'qursor.servers',
				JSON.stringify({ state: { tabs: [tab], active: 'ssh:t1', collapsed: [] }, version: 0 }),
			);
		});
		await installMockBridge(page, { serverBook: book });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		await expect(page.getByText('Not connected since Qursor was restarted.')).toBeVisible();
		const opens = () =>
			page.evaluate(
				() =>
					(window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === 'server_open_shell')
						.length,
			);
		expect(await opens()).toBe(0);

		await page.getByTestId('server-reconnect').click();
		await expect(page.getByTestId('server-terminal')).toBeVisible();
		await expect.poll(opens).toBe(1);
	});

	test('@smoke a server dragged onto a folder moves there, and the menu does the same', async ({
		page,
	}) => {
		const twoFolders: ServerBook = {
			...book,
			folders: [...book.folders, { id: 'f2', name: 'Infra', projectId: null }],
		};
		await installMockBridge(page, { serverBook: twoFolders });
		await page.goto('/');
		await page.getByTestId('mode-servers').click();
		const saves = () =>
			page.evaluate(() =>
				(window.__FACTORAI_TEST_CALLS__ ?? [])
					.filter((c) => c.name === 'save_server')
					.map((c) => c.args as { input: Record<string, unknown> }),
			);

		const from = await page.getByTestId('server-row-web01').boundingBox();
		const to = await page.getByTestId('server-drop-folder:f2').boundingBox();
		if (!from || !to) throw new Error('no box');
		await page.mouse.move(from.x + 20, from.y + from.height / 2);
		await page.mouse.down();
		// dnd-kit reads `over` a move behind: aim twice, a pixel apart.
		await page.mouse.move(to.x + 30, to.y + 10, { steps: 8 });
		await page.mouse.move(to.x + 31, to.y + 10);
		await expect(page.getByTestId('drag-chip')).toHaveText('web01');
		await page.mouse.up();
		await expect.poll(async () => (await saves()).length).toBe(1);
		const [{ input }] = await saves();
		expect(input).toMatchObject({ id: 's1', folderId: 'f2', name: 'web01' });
		expect(input).not.toHaveProperty('password');

		await page.getByTestId('server-row-db01').hover();
		await page.getByRole('button', { name: 'db01 actions', exact: true }).click();
		await page.getByTestId('server-move').click();
		await page.getByRole('menuitem', { name: 'No folder' }).click();
		await expect.poll(async () => (await saves()).length).toBe(2);
		expect((await saves())[1].input).toMatchObject({ id: 's2', folderId: null });
	});
});
