import { expect, test } from '@playwright/test';
import {
	FOO_ID,
	fixtureOneProjectOneSession,
	fixtureWithGraph,
	installMockBridge,
	type TestFixture,
} from './fixtures';

/**
 * Git mode (specs/05-features.md F35): the `Sessions | Git` switch, the Git tab
 * strip and the history view. Writes are mocked; what is asserted is which op
 * the click asked for.
 */

const PROJECT = `/#/projects/${FOO_ID}`;

/** The graph fixture plus the refs the sidebar and action bar read. */
function fixtureWithRefs(extra: Partial<TestFixture> = {}): TestFixture {
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
		...extra,
	};
}

/** `fixtureWithRefs` plus a status with these changes, and the project root. */
function fixtureWithChanges(
	changes: { rel: string; group: 'staged' | 'unstaged' }[],
	extra: Partial<TestFixture> = {},
): { fx: TestFixture; root: string } {
	const base = fixtureWithRefs(extra);
	const root = base.projects?.[0]?.realPath ?? '';
	return {
		root,
		fx: {
			...base,
			gitStatuses: {
				[root]: {
					repoRoot: root,
					branch: 'main',
					head: 'a'.repeat(40),
					total: changes.length,
					truncated: false,
					changes: changes.map((c) => ({
						path: `${root}/${c.rel}`,
						relPath: c.rel,
						group: c.group,
						kind: 'modified' as const,
						oldRelPath: null,
						additions: 1,
						deletions: 0,
						isBinary: false,
					})),
				},
			},
		},
	};
}

const gitOps = (page: import('@playwright/test').Page) =>
	page.evaluate(() =>
		(window.__FACTORAI_TEST_CALLS__ ?? [])
			.filter((c) => c.name === 'git_op')
			.map((c) => (c.args as { op: unknown }).op),
	);
const callCount = (page: import('@playwright/test').Page, name: string) =>
	page.evaluate(
		(n) => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n).length,
		name,
	);

test.describe('git-mode', () => {
	test('@smoke switching modes hides the session strip and keeps the terminal', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.locator('aside').getByText('foo').click();
		await page.getByText('Refactor the auth middleware').click();
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await expect(page.getByTestId('session-tabs')).toBeVisible();

		await page.getByRole('tab', { name: /^Git/ }).click();
		await expect(page.getByTestId('session-tabs')).toBeHidden();
		await expect(page.getByTestId('git-tab')).toBeVisible();
		await expect(page.getByTestId('git-tabs').getByTestId('tab-group-chip')).toContainText('foo');

		await page.getByRole('tab', { name: /^Sessions/ }).click();
		await expect(page.getByTestId('session-tabs')).toBeVisible();
		await expect(
			page.getByTestId('session-tabs').getByRole('tab', { selected: true }),
		).toBeVisible();
		// Mounted all along, not remounted.
		await expect(page.locator('.xterm')).not.toHaveCount(0);
		await expect(page.locator('.xterm:visible')).toBeVisible();
	});

	test('@smoke a folder that is no project opens as a repository tab', async ({ page }) => {
		await installMockBridge(page, { ...fixtureWithRefs(), folderPick: '/home/me/other-repo' });
		await page.goto('/');
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByRole('button', { name: 'Open Git tab' }).click();
		await page.getByTestId('git-open-repository').click();
		await expect(page.getByTestId('git-tabs').getByTestId('tab-group-chip')).toContainText(
			'other-repo',
		);
	});

	test('@smoke history rows render and double-clicking a branch checks it out', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithRefs());
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();

		await expect(page.getByTestId('commit-row')).toHaveCount(5);
		await page.getByTestId('git-branch').filter({ hasText: 'main' }).dblclick();
		await expect
			.poll(() =>
				page.evaluate(() =>
					(window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === 'git_op'),
				),
			)
			.toEqual([
				expect.objectContaining({
					args: expect.objectContaining({ op: { kind: 'checkout', branch: 'main' } }),
				}),
			]);
		await expect(page.getByTestId('op-log')).toContainText('$ git checkout');
	});

	test('@smoke a project that is not a repository says so', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await expect(page.getByText('Not a git repository.')).toBeVisible();
	});

	test('@smoke a rejected push opens the error dialog and Pull and push pulls', async ({
		page,
	}) => {
		await installMockBridge(
			page,
			fixtureWithRefs({
				gitOpResult: {
					command: 'git push',
					ok: false,
					stdout: '',
					stderr:
						"! [rejected]        main -> main (fetch first)\nerror: failed to push some refs to 'origin'",
				},
			}),
		);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByRole('button', { name: 'Push' }).click();
		const dialog = page.getByTestId('git-error-dialog');
		await expect(dialog).toContainText('Push rejected');
		await expect(dialog).toContainText('failed to push some refs');
		await dialog.getByRole('button', { name: 'Pull and push' }).click();
		await expect
			.poll(() =>
				page.evaluate(() =>
					(window.__FACTORAI_TEST_CALLS__ ?? [])
						.filter((c) => c.name === 'git_op')
						.map((c) => (c.args as { op: { kind: string } }).op.kind),
				),
			)
			.toContain('pull');
	});

	test('@smoke a missing git is shown in the op log', async ({ page }) => {
		await installMockBridge(
			page,
			fixtureWithRefs({ gitOpFails: { kind: 'NotFound', message: 'git not found on PATH' } }),
		);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByRole('button', { name: 'Fetch' }).click();
		await expect(page.getByTestId('git-error-dialog')).toContainText(
			'git is not installed or not on PATH',
		);
		await page.getByTestId('git-error-dialog').getByRole('button', { name: 'Close' }).click();
		await expect(page.getByTestId('op-log')).toContainText('git not found on PATH');
		// History still works.
		await expect(page.getByTestId('commit-row')).toHaveCount(5);
	});
});

test.describe('git-mode local changes', () => {
	test('@smoke staging a file records a stage op', async ({ page }) => {
		const { fx, root } = fixtureWithChanges([{ rel: 'a.ts', group: 'unstaged' }]);
		await installMockBridge(page, fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('git-local-changes').click();
		await page
			.getByTestId('unstaged-list')
			.getByRole('button', { name: 'Stage', exact: true })
			.click();
		await expect.poll(() => gitOps(page)).toEqual([{ kind: 'stage', paths: [`${root}/a.ts`] }]);
	});

	test('@smoke committing records the message', async ({ page }) => {
		const { fx } = fixtureWithChanges([{ rel: 'a.ts', group: 'staged' }]);
		await installMockBridge(page, fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('git-local-changes').click();
		await page.getByLabel('Commit subject').fill('fix: x');
		await page.getByRole('button', { name: 'Commit on main' }).click();
		await expect
			.poll(() => gitOps(page))
			.toEqual([{ kind: 'commit', message: 'fix: x', amend: false }]);
	});

	test('@smoke a cancelled discard records nothing', async ({ page }) => {
		const { fx } = fixtureWithChanges([{ rel: 'a.ts', group: 'unstaged' }]);
		await installMockBridge(page, fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('git-local-changes').click();
		await page.getByRole('button', { name: 'Discard' }).click();
		await expect(page.getByText('Are you sure?')).toBeVisible();
		await page.getByRole('button', { name: 'Cancel' }).click();
		await expect(page.getByText('Are you sure?')).toBeHidden();
		expect(await callCount(page, 'git_op')).toBe(0);
	});
});

test.describe('git-mode merge and conflicts', () => {
	const conflicted = () => {
		const { fx, root } = fixtureWithChanges([]);
		return {
			root,
			fx: {
				...fx,
				gitRepoStates: {
					[root]: { op: 'merge' as const, conflicted: [`${root}/a.ts`], theirs: 'b'.repeat(40) },
				},
			},
		};
	};

	test('@smoke a conflicted merge shows the banner and never starts an agent on its own', async ({
		page,
	}) => {
		await installMockBridge(page, conflicted().fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await expect(page.getByTestId('conflict-banner')).toContainText('a.ts');
		await page.waitForTimeout(500);
		expect(await callCount(page, 'start_session')).toBe(0);
	});

	test('@smoke resolve with agent starts a session, shows the origin strip and goes back', async ({
		page,
	}) => {
		await installMockBridge(page, conflicted().fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByRole('button', { name: /Resolve with agent/ }).click();
		await expect.poll(() => callCount(page, 'start_session')).toBe(1);
		await expect(page.getByTestId('git-origin-strip')).toBeVisible();
		await expect(page.getByTestId('git-mode-view')).toBeHidden();
		await page.getByRole('button', { name: 'Back to Git' }).click();
		await expect(page.getByTestId('git-mode-view')).toBeVisible();
	});

	test('@smoke merge asks first, and stash and continue stashes then merges', async ({ page }) => {
		const { fx } = fixtureWithChanges([{ rel: 'a.ts', group: 'unstaged' }]);
		await installMockBridge(page, fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByRole('button', { name: /^Merge/ }).click();
		await page.getByRole('menuitem', { name: 'side' }).click();
		await expect(page.getByRole('dialog')).toContainText('from side enter main');
		expect(await callCount(page, 'git_op')).toBe(0);
		await page.getByRole('button', { name: 'Stash and continue' }).click();
		await expect
			.poll(() => gitOps(page))
			.toEqual([
				{ kind: 'stashPush', message: null },
				{ kind: 'merge', branch: 'side' },
			]);
	});
});

test.describe('git-mode quick agent actions', () => {
	const undo = async (page: import('@playwright/test').Page, answer: string) => {
		await installMockBridge(page, fixtureWithRefs({ gitAskAgent: answer }));
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('commit-row').first().click({ button: 'right' });
		await page.getByRole('menuitem', { name: /Undo this commit safely/ }).click();
	};

	test('@smoke undo commit offers Run for an allowed proposal', async ({ page }) => {
		await undo(page, '# undo it\ngit revert abc1234');
		await expect(page.getByTestId('agent-popover')).toContainText('undo it');
		await expect(page.getByRole('button', { name: 'Run', exact: true })).toBeVisible();
		await page.getByRole('button', { name: 'Run', exact: true }).click();
		await expect.poll(() => callCount(page, 'git_run_proposed')).toBe(1);
	});

	test('@smoke undo commit hides Run for a refused line but keeps Open in a session', async ({
		page,
	}) => {
		await undo(page, 'git reset --hard HEAD~1');
		await expect(page.getByRole('button', { name: 'Open in a session' })).toBeVisible();
		await expect(page.getByRole('button', { name: 'Run', exact: true })).toBeHidden();
	});

	test('@smoke write message fills subject and body', async ({ page }) => {
		const { fx } = fixtureWithChanges([{ rel: 'a.ts', group: 'staged' }], {
			gitAskAgent: 'feat: add x\n\nbecause y',
		});
		await installMockBridge(page, fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('git-local-changes').click();
		await page.getByRole('button', { name: '✦ Write message' }).click();
		await expect(page.getByLabel('Commit subject')).toHaveValue('feat: add x');
		await expect(page.getByLabel('Commit body')).toHaveValue('because y');
		await expect(page.getByRole('button', { name: '✦ Regenerate' })).toBeVisible();
	});

	test('@smoke quick buttons are hidden for a Codex project', async ({ page }) => {
		const { fx } = fixtureWithChanges([{ rel: 'a.ts', group: 'staged' }], {
			settings: { agentDefault: 'codex' },
		});
		await installMockBridge(page, fx);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('git-local-changes').click();
		await expect(page.getByTestId('commit-box')).toBeVisible();
		await expect(page.getByRole('button', { name: '✦ Write message' })).toHaveCount(0);
		await expect(page.getByLabel('Ask the agent')).toHaveCount(0);
	});
});
