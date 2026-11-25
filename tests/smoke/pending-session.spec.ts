import { type Page, expect, test } from '@playwright/test';
import { ALPHA_ID, fixtureTwoProjectsManySessions, installMockBridge } from './fixtures';

/**
 * A session that is live but not indexed yet (specs/05-features.md F6,
 * "Reachability before indexing").
 *
 * `claude` writes no transcript until you send it a message, so `list_sessions`
 * has no row for a session you just started — and both lists that show sessions
 * used to say "no sessions" about a project with a running PTY. The sidebar was
 * the worse half: it is where you look for a session *under its project*, and
 * it showed nothing at all.
 */
const NEW_ID = 'aaaaaaaa-0000-4000-8000-00000000000a';

function fixture() {
	return { ...fixtureTwoProjectsManySessions(), newSessionId: NEW_ID };
}

test.describe('a session started but not yet indexed', () => {
	test('@smoke appears under its project in the sidebar', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto('/');

		await page.getByRole('button', { name: 'Expand alpha' }).click();
		await page.getByRole('button', { name: 'New session in alpha' }).click();

		await expect(page).toHaveURL(new RegExp(`sessions/${NEW_ID}$`));
		const rows = page.getByTestId(`sidebar-sessions-${ALPHA_ID}`).getByRole('link');
		// Above the one indexed session, and it is the row the route is on.
		await expect(rows.first()).toContainText('New session');
		await expect(rows.nth(1)).toContainText('Alpha only task');
	});

	test('@smoke appears on the project page, and is reachable from it', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto(`/#/projects/${ALPHA_ID}`);

		await page.getByRole('button', { name: 'New session', exact: true }).click();
		await expect(page).toHaveURL(new RegExp(`sessions/${NEW_ID}$`));

		// Navigate away and back: the row is the only way back to a session that
		// has written nothing.
		await page.goBack();
		const row = page.getByRole('link', { name: /New session/ });
		await expect(row).toBeVisible();
		await row.click();
		await expect(page).toHaveURL(new RegExp(`sessions/${NEW_ID}$`));
	});
});

/**
 * The other half of F6: once the watcher has indexed the transcript it emits
 * `sessions:changed`, and every list showing that session has to notice.
 *
 * These drive the event through the mock bridge rather than waiting on a poll,
 * because that is the mechanism — the polls are only a net, and one of the two
 * surfaces below has none at all.
 */
test.describe('when the index catches up', () => {
	test('@smoke the sidebar swaps the pending row for the indexed session', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto('/');

		await page.getByRole('button', { name: 'Expand alpha' }).click();
		await page.getByRole('button', { name: 'New session in alpha' }).click();
		const rows = page.getByTestId(`sidebar-sessions-${ALPHA_ID}`).getByRole('link');
		await expect(rows.first()).toContainText('New session');

		await indexSession(page, 'Wire up the exporter');

		await expect(rows.first()).toContainText('Wire up the exporter');
		await expect(rows.filter({ hasText: 'New session' })).toHaveCount(0);
	});

	test('@smoke the tab takes the title claude derived', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto('/');

		await page.getByRole('button', { name: 'Expand alpha' }).click();
		await page.getByRole('button', { name: 'New session in alpha' }).click();

		// The strip has no poll of its own, so before this fix the tab kept the
		// short id it was born with for as long as the session stayed open.
		const tab = page.getByTestId('session-tabs').getByRole('tab');
		await expect(tab).toContainText(NEW_ID.slice(0, 8));

		await indexSession(page, 'Wire up the exporter');

		await expect(tab).toContainText('Wire up the exporter');
	});
});

/** Index the new session in the mock's data, then say so the way the watcher
 *  does. Two steps, exactly like the backend: the row appears, then the event
 *  tells the renderer to look again. */
async function indexSession(page: Page, title: string): Promise<void> {
	await page.evaluate(
		({ projectId, sessionId, sessionTitle }) => {
			const fx = window.__FACTORAI_TEST__;
			if (!fx?.sessionsByProject) throw new Error('no fixture to index into');
			fx.sessionsByProject[projectId] = [
				{
					id: sessionId,
					projectId,
					title: sessionTitle,
					createdAt: Date.now(),
					updatedAt: Date.now(),
					turnCount: 1,
					cwd: null,
					subagentOf: null,
				},
				...(fx.sessionsByProject[projectId] ?? []),
			];
			window.__FACTORAI_EMIT__?.('sessions:changed', { projectId, sessionIds: [sessionId] });
		},
		{ projectId: ALPHA_ID, sessionId: NEW_ID, sessionTitle: title },
	);
}

test('@smoke an OpenCode delegate starts only after the user approves its brief', async ({
	page,
}) => {
	const request = {
		delegationId: NEW_ID,
		projectId: ALPHA_ID,
		agent: 'opencode' as const,
		cwd: '/workspace/alpha',
		prompt: 'Review the fixture diff',
	};
	await installMockBridge(page, {
		...fixture(),
		delegationRequest: request,
		opencodeCli: { installed: true, binaryPath: '/usr/local/bin/opencode', version: '1.18.30' },
	});
	await page.goto('/');
	await page.getByRole('button', { name: 'Expand alpha' }).click();
	const emit = () =>
		page.evaluate((payload) => window.__FACTORAI_EMIT__?.('agent-session:start', payload), request);
	const spawnCalls = () =>
		page.evaluate(
			() => window.__FACTORAI_TEST_CALLS__?.filter((c) => c.name === 'terminal_spawn') ?? [],
		);
	await emit();
	const dialog = page.getByRole('dialog', { name: 'Delegate to OpenCode' });
	await expect(dialog.getByText(request.prompt)).toBeVisible();
	await expect(dialog.getByText(request.cwd)).toBeVisible();
	await dialog.getByRole('button', { name: 'Decline' }).click();
	await expect(dialog).toHaveCount(0);
	expect(await spawnCalls()).toEqual([]);

	await emit();
	await dialog.getByRole('button', { name: 'Start session' }).click();
	await expect.poll(spawnCalls).toContainEqual({
		name: 'terminal_spawn',
		args: {
			opts: expect.objectContaining({
				agent: 'opencode',
				sessionId: NEW_ID,
				projectId: ALPHA_ID,
				initialPrompt: request.prompt,
				cwd: request.cwd,
			}),
		},
	});
	const row = page.getByTestId(`sidebar-sessions-${ALPHA_ID}`).getByRole('link').first();
	await expect(row.getByRole('img', { name: 'OpenCode' })).toBeVisible();
	await expect(page.getByTestId('session-tabs').getByRole('tab')).toHaveCount(0);
});

test('@smoke starts OpenCode in the background when delegation confirmation is off', async ({
	page,
}) => {
	await installMockBridge(page, fixture());
	await page.goto('/?settings=confirmations');
	const confirmation = page.getByRole('switch', {
		name: 'Ask before starting a delegated session',
	});
	await expect(confirmation).toBeChecked();
	await confirmation.click();
	await page.getByTestId('settings-save').click();
	await expect(confirmation).toHaveCount(0);
	const calls = () => page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
	expect(await calls()).toContainEqual({
		name: 'set_setting',
		args: { key: 'confirmAgentDelegations', value: 'off' },
	});

	await page.getByRole('button', { name: 'Expand alpha' }).click();
	await page.evaluate(
		({ projectId, id }) =>
			window.__FACTORAI_EMIT__?.('agent-session:start', {
				delegationId: id,
				autoApproved: true,
				projectId,
				agent: 'opencode',
				cwd: '/workspace/alpha',
				prompt: 'Review the fixture diff',
			}),
		{ projectId: ALPHA_ID, id: NEW_ID },
	);
	const row = page.getByTestId(`sidebar-sessions-${ALPHA_ID}`).getByRole('link').first();
	await expect(row.getByRole('img', { name: 'OpenCode' })).toBeVisible();
	await expect(page.getByRole('dialog', { name: 'Delegate to OpenCode' })).toHaveCount(0);
	await expect(page.getByTestId('session-tabs').getByRole('tab')).toHaveCount(0);
	expect((await calls()).some((call) => call.name === 'resolve_delegation')).toBe(false);
});
