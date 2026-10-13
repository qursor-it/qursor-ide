import { expect, test, type Page } from '@playwright/test';
import {
	FOO_ID,
	fixtureWithFileTree,
	fixtureWithOwnPullRequest,
	fixtureWithPullRequests,
	fixtureWithPullRequestsViaRemote,
	fixtureWithPullRequestsSignedOut,
	installMockBridge,
} from './fixtures';

/**
 * The panel's PRs tab (specs/05-features.md F31, ADR-0077).
 *
 * The Rust half is tested against fixtures of `gh`'s real output; what is left
 * for the browser is what the renderer decides: which empty state, what the
 * list marks and what it leaves unmarked, what the filter asks for, and that a
 * file opens the right two sides.
 */

const PROJECT = `/#/projects/${FOO_ID}`;

// The fixture's own SHAs: `sha(52)` is the merge base and `sha(51)` the head.
const MERGE_BASE_SHORT = '5252abc';
const HEAD_SHORT = '5151abc';

/** One side's document text. The `:not()`s drop the deleted/inserted view zones
 *  Monaco injects to line the sides up, which carry the same `view-lines` class
 *  and are not part of the document. */
function side(page: Page, which: 'original' | 'modified') {
	return page
		.getByTestId('diff-view-editor')
		.locator(`.editor.${which} .view-lines:not(.line-delete):not(.line-insert)`);
}

async function openPrs(page: Page) {
	await page.getByRole('button', { name: 'Toggle file tree' }).click();
	await expect(page.getByTestId('file-tree-panel')).toBeVisible();
	await page.getByRole('tab', { name: 'PRs' }).click();
}

function calls(page: Page, name: string) {
	return page.evaluate(
		(n) => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n),
		name,
	);
}

/** The `<sha>:<relPath>` of every blob read so far. */
async function blobReads(page: Page) {
	return (await calls(page, 'gh_pr_blob')).map((c) => `${c.args?.sha}:${c.args?.relPath}`);
}

test.describe('pull requests tab', () => {
	test('@smoke says how to install gh when there is none', async ({ page }) => {
		// No `ghStatuses` at all: the browser-only default, a machine without gh.
		await installMockBridge(page, fixtureWithFileTree());
		await page.goto(PROJECT);
		await openPrs(page);

		const gate = page.getByTestId('pr-gate-not-installed');
		await expect(gate).toContainText('Install GitHub CLI');
		await expect(gate).toContainText('Then restart Qursor IDE');
		// The command is the platform's, and Chromium reports whichever host runs
		// the suite, so only its shape is asserted (Linux prints none).
		const command = gate.getByTestId('pr-command');
		if (await command.count()) {
			await expect(command).toHaveText(/winget install GitHub\.cli|brew install gh/);
		}
	});

	test('@smoke asks for gh auth login when there is no login, and retries', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequestsSignedOut());
		await page.goto(PROJECT);
		await openPrs(page);

		const gate = page.getByTestId('pr-gate-not-authenticated');
		await expect(gate.getByTestId('pr-command')).toHaveText('gh auth login');
		await expect(gate.getByRole('button', { name: 'Copy command' })).toBeVisible();

		await gate.getByTestId('pr-retry').click();
		await expect
			.poll(async () => (await calls(page, 'gh_status')).some((c) => c.args?.refresh === true))
			.toBe(true);
	});

	test('names the remote when the repository was not read from origin', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequestsViaRemote());
		await page.goto(PROJECT);
		await openPrs(page);

		await expect(page.getByTestId('pr-repo')).toContainText('acme/widgets');
		await expect(page.getByTestId('pr-remote')).toHaveText('· via remote xplorer');
	});

	test('@smoke lists the pull requests and marks only what earns a mark', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);

		// Open is the default filter: #13 is merged and is not listed.
		await expect(page.getByTestId('pr-row-11')).toContainText('feat: add the widget switcher');
		await expect(page.getByTestId('pr-row-12')).toContainText('wip: rewrite the renderer');
		await expect(page.getByTestId('pr-row-13')).toHaveCount(0);
		await expect(page.getByTestId('pr-repo')).toHaveText('acme/widgets');
		// Read from `origin`: nothing to explain.
		await expect(page.getByTestId('pr-remote')).toHaveCount(0);

		// The draft chip is on #12 only.
		await expect(page.getByTestId('pr-row-12').getByTestId('pr-draft')).toBeVisible();
		await expect(page.getByTestId('pr-row-11').getByTestId('pr-draft')).toHaveCount(0);
		// A failing run is marked on #12.
		await expect(page.getByTestId('pr-row-12').getByTestId('pr-checks-failure')).toBeVisible();
		await expect(page.getByTestId('pr-row-11').getByTestId('pr-checks-success')).toBeVisible();
		// **The blue marker is on #11 alone** (DESIGN.md, The One Accent Rule).
		await expect(page.getByTestId('pr-review-requested')).toHaveCount(1);
		await expect(page.getByTestId('pr-row-11').getByTestId('pr-review-requested')).toBeVisible();
		await expect(page.getByTestId('pr-row-11')).toContainText('Review required');
	});

	test('@smoke asks gh for the state the filter names', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);
		await expect(page.getByTestId('pr-row-11')).toBeVisible();

		await page.getByTestId('pr-filter').click();
		await page.getByTestId('pr-filter-merged').click();

		await expect(page.getByTestId('pr-row-13')).toContainText('fix: stop the leak on close');
		await expect(page.getByTestId('pr-row-11')).toHaveCount(0);
		const states = (await calls(page, 'gh_pr_list')).map((c) => c.args?.state);
		expect(states).toContain('open');
		expect(states).toContain('merged');
	});

	test('@smoke opens a pull request, reads its overview, and goes back', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);

		await page.getByTestId('pr-row-11').click();
		const detail = page.getByTestId('pr-detail');
		await expect(detail.getByTestId('pr-title')).toHaveText('feat: add the widget switcher');
		await expect(detail).toContainText('#11');
		await expect(detail).toContainText('main ← feat/pr-11');
		// Files is the default sub-tab.
		await expect(page.getByTestId('pr-files')).toBeVisible();

		await detail.getByRole('tab', { name: 'Overview' }).click();
		await expect(page.getByTestId('pr-body')).toContainText('Adds a switcher');
		const checks = page.getByTestId('pr-checks');
		await expect(checks).toContainText('JS - lint, types, tests');
		await expect(checks).toContainText('Quality');
		await expect(checks).toContainText('Rust - clippy, tests');
		await expect(page.getByTestId('pr-reviews')).toContainText('bob');
		await expect(page.getByTestId('pr-reviews')).toContainText('Commented');
		await expect(page.getByTestId('pr-requests')).toContainText('alice-reviewer, @core');
		await expect(page.getByTestId('pr-labels')).toContainText('enhancement');
		await expect(page.getByTestId('pr-mergeable')).toHaveText('No conflicts');

		await page.getByRole('button', { name: 'Back to pull requests' }).click();
		await expect(page.getByTestId('pr-list')).toBeVisible();
		await expect(page.getByTestId('pr-detail')).toHaveCount(0);
	});

	test('@smoke opens the pull request on GitHub through the shell', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();

		await page.getByTestId('pr-open-github').click();
		await expect
			.poll(async () => (await calls(page, 'openExternally')).map((c) => c.args?.path))
			.toEqual(['https://github.com/acme/widgets/pull/11']);
	});

	test('@smoke diffs a modified file, with both sides and the PR label', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();

		await page.getByTestId('pr-files').getByTitle('src/switcher.ts').click();

		await expect(side(page, 'original')).toContainText('export const switcher = 1;');
		await expect(side(page, 'modified')).toContainText('export const switcher = 2;');
		await expect(page.getByText(`PR #11 · ${MERGE_BASE_SHORT} ↔ ${HEAD_SHORT}`)).toBeVisible();
		await expect(page.getByTestId('viewer-read-only')).toHaveText('pull request — read-only');
		// The old side is read at the merge base, the new one at the head.
		const reads = await blobReads(page);
		expect(reads.some((b) => b.startsWith('5252') && b.endsWith(':src/switcher.ts'))).toBe(true);
		expect(reads.some((b) => b.startsWith('5151') && b.endsWith(':src/switcher.ts'))).toBe(true);
	});

	test('@smoke reads a rename from its old path on the left', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();

		await page
			.getByTestId('pr-files')
			.getByTitle(/src\/new-name\.ts/)
			.click();

		await expect(side(page, 'original')).toContainText('"old"');
		await expect(side(page, 'modified')).toContainText('"new"');
		const reads = await blobReads(page);
		expect(reads.some((b) => b.startsWith('5252') && b.endsWith(':src/old-name.ts'))).toBe(true);
	});

	test('@smoke gives an added file an empty left side without asking for one', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();

		await page.getByTestId('pr-files').getByTitle('src/added.ts').click();

		await expect(side(page, 'modified')).toContainText('export const added = true;');
		await expect(side(page, 'original')).not.toContainText('added');
		const reads = await blobReads(page);
		expect(reads.some((b) => b.startsWith('5252') && b.endsWith(':src/added.ts'))).toBe(false);
	});

	test('@smoke says so when a state has no pull requests', async ({ page }) => {
		const fixture = fixtureWithPullRequests();
		fixture.ghPrLists = {};
		await installMockBridge(page, fixture);
		await page.goto(PROJECT);
		await openPrs(page);

		await expect(page.getByTestId('pr-list')).toContainText('No open pull requests.');
	});
});

test.describe('review with agent', () => {
	const NEW_ID = '11111111-2222-4333-8444-555555555555';

	async function openPr(page: Page) {
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();
		await expect(page.getByTestId('pr-title')).toBeVisible();
	}

	test('@smoke starts a fresh session with the review prompt and opens it', async ({ page }) => {
		await installMockBridge(page, { ...fixtureWithPullRequests(), newSessionId: NEW_ID });
		await openPr(page);
		await page.getByTestId('pr-review-agent').click();

		await expect(page).toHaveURL(new RegExp(`/projects/${FOO_ID}/sessions/${NEW_ID}`));
		await expect(page.getByTestId('session-tabs').getByRole('tab')).toHaveCount(1);

		const start = (await calls(page, 'start_session'))[0];
		expect(start?.args?.fresh).toBe(true);
		const spawn = (await calls(page, 'terminal_spawn'))[0];
		const opts = spawn?.args?.opts as { initialPrompt?: string; cwd?: string };
		expect(opts.initialPrompt).toContain('gh pr diff 11 --repo github.com/acme/widgets');
		expect(opts.initialPrompt).not.toContain('feat: add the widget switcher');
		expect(opts.initialPrompt).not.toContain('keyboard friendly');
		expect(await calls(page, 'set_session_worktree')).toHaveLength(0);
	});

	test('@smoke offers Codex when it is installed', async ({ page }) => {
		const fixture = { ...fixtureWithPullRequests(), newSessionId: NEW_ID };
		fixture.claudeCli = { installed: true, binaryPath: '/usr/bin/claude', version: '2.1.0' };
		fixture.codexCli = { installed: true, binaryPath: '/usr/bin/codex', version: '0.44.0' };
		await installMockBridge(page, fixture);
		await openPr(page);
		await page.getByTestId('pr-review-agent').click();
		await page.getByTestId('pr-review-agent-codex').click();

		await expect(page).toHaveURL(new RegExp(`/sessions/${NEW_ID}`));
		const spawn = (await calls(page, 'terminal_spawn'))[0];
		expect((spawn?.args?.opts as { agent?: string }).agent).toBe('codex');
	});

	test('@smoke runs in the worktree that has the head checked out', async ({ page }) => {
		const fixture = { ...fixtureWithPullRequests(), newSessionId: NEW_ID };
		const root = fixture.projects?.[0]?.realPath ?? '';
		const wt = '/home/alice/code/worktrees/pr-11';
		fixture.gitWorktrees = {
			[root]: [
				{
					path: root,
					name: null,
					branch: 'main',
					head: 'a'.repeat(40),
					isMain: true,
					locked: false,
					prunable: false,
					exists: true,
				},
				{
					path: wt,
					name: 'pr-11',
					branch: 'review-11',
					head: fixture.ghPrDetails?.['github.com/acme/widgets#11']?.headRefOid ?? null,
					isMain: false,
					locked: false,
					prunable: false,
					exists: true,
				},
			],
		};
		await installMockBridge(page, fixture);
		await openPr(page);
		await page.getByTestId('pr-review-agent').click();

		await expect(page).toHaveURL(new RegExp(`/sessions/${NEW_ID}`));
		const spawn = (await calls(page, 'terminal_spawn'))[0];
		const opts = spawn?.args?.opts as { cwd?: string; initialPrompt?: string };
		expect(opts.cwd).toBe(wt);
		expect(opts.initialPrompt).toContain('checked out in the current directory');
		const set = (await calls(page, 'set_session_worktree'))[0];
		expect(set?.args?.path).toBe(wt);
	});
});

test.describe('checking a pull request out', () => {
	const NEW_ID = '11111111-2222-4333-8444-555555555555';

	/** The fixture with the project's main checkout listed, as a repository has. */
	function withMain() {
		const fixture = { ...fixtureWithPullRequests(), newSessionId: NEW_ID };
		const root = fixture.projects?.[0]?.realPath ?? '';
		fixture.gitWorktrees = {
			[root]: [
				{
					path: root,
					name: null,
					branch: 'main',
					head: 'a'.repeat(40),
					isMain: true,
					locked: false,
					prunable: false,
					exists: true,
				},
			],
		};
		return { fixture, root };
	}

	async function openPr(page: Page) {
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();
		await expect(page.getByTestId('pr-title')).toBeVisible();
	}

	test('@smoke asks, adds the worktree, and turns into Open checkout', async ({ page }) => {
		const { fixture, root } = withMain();
		await installMockBridge(page, fixture);
		await openPr(page);
		await expect(page.getByTestId('pr-open-checkout')).toHaveCount(0);

		await page.getByTestId('pr-checkout').click();
		const dialog = page.getByTestId('pr-checkout-dialog');
		await expect(dialog).toBeVisible();
		await expect(page.getByTestId('pr-checkout-path')).toContainText('-pr-11');
		await expect(page.getByTestId('pr-checkout-branch')).toHaveText('pr/11');
		await expect(page.getByTestId('pr-checkout-fork')).toHaveCount(0);
		// Reading the plan writes nothing.
		expect(await calls(page, 'gh_pr_checkout')).toHaveLength(0);

		await page.getByTestId('pr-checkout-confirm').click();

		await expect(page.getByTestId('pr-open-checkout')).toBeVisible();
		await expect(dialog).toHaveCount(0);
		await expect(page.getByText('Checked out PR #11')).toBeVisible();
		const sent = await calls(page, 'gh_pr_checkout');
		expect(sent).toHaveLength(1);
		expect(sent[0]?.args).toMatchObject({
			projectPath: root,
			number: 11,
			headOid: fixture.ghPrDetails?.['github.com/acme/widgets#11']?.headRefOid,
			detached: false,
		});
	});

	test('@smoke warns when the code comes from a fork', async ({ page }) => {
		const { fixture } = withMain();
		const detail = fixture.ghPrDetails?.['github.com/acme/widgets#11'];
		if (!detail) throw new Error('fixture has no #11');
		fixture.ghPrDetails = {
			'github.com/acme/widgets#11': { ...detail, isCrossRepository: true, headOwner: 'mallory' },
		};
		await installMockBridge(page, fixture);
		await openPr(page);

		await page.getByTestId('pr-checkout').click();

		await expect(page.getByTestId('pr-checkout-fork')).toContainText(
			'This code comes from a fork (mallory). Running it can execute anything it contains.',
		);
	});

	test('@smoke offers detached when pr/N is somewhere else', async ({ page }) => {
		const { fixture, root } = withMain();
		fixture.ghPrCheckoutPlans = {
			[`${root}#11`]: { branchExists: true, branchAt: 'b'.repeat(40) },
		};
		await installMockBridge(page, fixture);
		await openPr(page);

		await page.getByTestId('pr-checkout').click();
		await expect(page.getByTestId('pr-checkout-diverged')).toContainText('bbbbbbb');
		await page.getByTestId('pr-checkout-confirm').click();

		await expect(page.getByTestId('pr-open-checkout')).toBeVisible();
		expect((await calls(page, 'gh_pr_checkout'))[0]?.args?.detached).toBe(true);
	});

	test('@smoke keeps the dialog and adds nothing when the checkout fails', async ({ page }) => {
		const { fixture } = withMain();
		fixture.ghPrCheckoutFails = {
			kind: 'Remote',
			message: 'git could not fetch pull request #11: fatal: could not read Username',
		};
		await installMockBridge(page, fixture);
		await openPr(page);

		await page.getByTestId('pr-checkout').click();
		await page.getByTestId('pr-checkout-confirm').click();

		await expect(page.getByTestId('pr-checkout-error')).toContainText('could not read Username');
		await expect(page.getByTestId('pr-checkout-dialog')).toBeVisible();
		await expect(page.getByTestId('pr-open-checkout')).toHaveCount(0);
		expect(await calls(page, 'gh_pr_checkout')).toHaveLength(1);
	});

	test('@smoke starts a session in the checkout from Open checkout', async ({ page }) => {
		const { fixture } = withMain();
		await installMockBridge(page, fixture);
		await openPr(page);
		await page.getByTestId('pr-checkout').click();
		const path = (await page.getByTestId('pr-checkout-path').textContent()) ?? '';
		await page.getByTestId('pr-checkout-confirm').click();

		await page.getByTestId('pr-open-checkout').click();

		await expect(page).toHaveURL(new RegExp(`/projects/${FOO_ID}/sessions/${NEW_ID}`));
		expect((await calls(page, 'start_session'))[0]?.args?.fresh).toBe(true);
		const spawn = (await calls(page, 'terminal_spawn'))[0];
		expect((spawn?.args?.opts as { cwd?: string }).cwd).toBe(path);
		expect((await calls(page, 'set_session_worktree'))[0]?.args?.path).toBe(path);
	});

	test('@smoke reviews with the agent in the checkout once it exists', async ({ page }) => {
		const { fixture } = withMain();
		await installMockBridge(page, fixture);
		await openPr(page);
		await page.getByTestId('pr-checkout').click();
		const path = (await page.getByTestId('pr-checkout-path').textContent()) ?? '';
		await page.getByTestId('pr-checkout-confirm').click();
		await expect(page.getByTestId('pr-open-checkout')).toBeVisible();

		await page.getByTestId('pr-review-agent').click();

		await expect(page).toHaveURL(new RegExp(`/sessions/${NEW_ID}`));
		const spawn = (await calls(page, 'terminal_spawn'))[0];
		expect((spawn?.args?.opts as { cwd?: string }).cwd).toBe(path);
	});
});

test.describe('reviewing a pull request', () => {
	const SLUG = 'github.com/acme/widgets';
	const DRAFTS_KEY = 'factorai.reviewDrafts';

	async function openSwitcherDiff(page: Page) {
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();
		await page.getByTestId('pr-files').getByTitle('src/switcher.ts').click();
		await expect(side(page, 'modified')).toContainText('export const switcher = 2;');
	}

	/** Line 2 of the modified side, found by its text: Monaco recycles line
	 *  elements, so their order in the DOM is not the order of the lines. */
	function secondLine(page: Page) {
		return side(page, 'modified').locator('.view-line', { hasText: 'widgets' });
	}

	/** A review draft already in storage, as an earlier visit would have left it. */
	async function seedDraft(page: Page, head: string, over: Record<string, unknown> = {}) {
		await page.addInitScript(
			({ key, slug, head, over }) => {
				if (localStorage.getItem(key)) return;
				localStorage.setItem(
					key,
					JSON.stringify({
						state: {
							byPr: {
								[`${slug}#11`]: {
									headOid: head,
									body: '',
									event: 'COMMENT',
									comments: [
										{
											id: 'seeded-1',
											path: 'src/switcher.ts',
											side: 'RIGHT',
											line: 1,
											body: 'Rename this constant.',
											createdAt: 1,
										},
									],
									...over,
								},
							},
						},
						version: 1,
					}),
				);
			},
			{ key: DRAFTS_KEY, slug: SLUG, head, over },
		);
	}

	async function openReviewTab(page: Page) {
		await page.goto(PROJECT);
		await openPrs(page);
		await page.getByTestId('pr-row-11').click();
		await page.getByTestId('pr-review-tab').click();
		await expect(page.getByTestId('pr-review-panel')).toBeVisible();
	}

	test('@smoke draws the conversation already on GitHub, and not the outdated one', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openSwitcherDiff(page);

		const thread = page.getByTestId('review-thread');
		await expect(thread).toHaveCount(1);
		await expect(thread).toContainText('Why a constant here?');
		await expect(thread).toContainText('Because the tests read it.');
		await expect(thread).toContainText('bob');
		await expect(page.getByText('Stale remark')).toHaveCount(0);
		expect((await calls(page, 'gh_pr_comments')).length).toBeGreaterThan(0);
	});

	test('@smoke comments from the glyph margin: the typing lands, and the draft is pending', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openSwitcherDiff(page);

		await secondLine(page).hover();
		const glyph = page
			.getByTestId('diff-view-editor')
			.locator('.editor.modified .factorai-review-add');
		await expect(glyph).toBeVisible();
		await glyph.click();

		// **This is the view-zone focus check**: the keys go through the keyboard,
		// not into the field, so a zone that lost focus to the editor fails here.
		const text = page.getByTestId('review-composer-text');
		await expect(text).toBeFocused();
		// A real click must land in the field too, not on the text layer behind it.
		await text.click();
		await expect(text).toBeFocused();
		await page.keyboard.type('Please rename this.');
		await expect(text).toHaveValue('Please rename this.');

		await page.getByTestId('review-composer-add').click();
		const draft = page.getByTestId('review-draft');
		await expect(draft).toContainText('Pending');
		await expect(draft).toContainText('Please rename this.');
		await expect(page.getByTestId('review-composer')).toHaveCount(0);

		await page.getByTestId('pr-review-tab').click();
		await expect(page.getByTestId('pr-review-tab-badge')).toHaveText('1');
		await expect(page.getByTestId('pr-review-draft')).toContainText('src/switcher.ts:2');
	});

	test('@smoke comments from the keyboard on the cursor line', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openSwitcherDiff(page);

		await secondLine(page).click();
		await page.keyboard.press('Control+Alt+M');
		const text = page.getByTestId('review-composer-text');
		await expect(text).toBeFocused();
		await page.keyboard.type('Keyboard comment');
		await page.keyboard.press('Control+Enter');

		await expect(page.getByTestId('review-draft')).toContainText('Keyboard comment');
		await expect(page.getByTestId('review-composer')).toHaveCount(0);
	});

	test('@smoke makes a multi-line comment from a selection', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openSwitcherDiff(page);

		await secondLine(page).click();
		await page.keyboard.press('Home');
		await page.keyboard.press('Control+Home');
		await page.keyboard.press('Shift+ArrowDown');
		await page.keyboard.press('Shift+End');
		await page.keyboard.press('Control+Alt+M');
		await expect(page.getByTestId('review-composer')).toContainText('Lines 1–2');
		await expect(page.getByTestId('review-composer-text')).toBeFocused();
		await page.keyboard.type('Both of these');
		await page.keyboard.press('Control+Enter');

		await expect(page.getByTestId('review-draft')).toContainText('Lines 1–2');
		await page.getByTestId('pr-review-tab').click();
		await expect(page.getByTestId('pr-review-draft')).toContainText('src/switcher.ts:1–2');
	});

	test('@smoke keeps typing out of the editor, and Escape closes the composer', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openSwitcherDiff(page);

		await secondLine(page).click();
		await page.keyboard.press('Control+Alt+M');
		await expect(page.getByTestId('review-composer-text')).toBeFocused();
		await page.keyboard.type('abc');
		// Ctrl+A in the box selects the box's text, not the file's.
		await page.keyboard.press('Control+A');
		await page.keyboard.type('z');
		await expect(page.getByTestId('review-composer-text')).toHaveValue('z');

		await page.keyboard.press('Escape');
		await expect(page.getByTestId('review-composer')).toHaveCount(0);
		await expect(page.getByTestId('review-draft')).toHaveCount(0);
	});

	test('@smoke refuses a line that is not part of the diff', async ({ page }) => {
		// Line 2 of the modified side is outside the only range.
		const fixture = fixtureWithPullRequests();
		const root = fixture.projects?.[0]?.realPath ?? '';
		const listing = fixture.ghPrFiles?.[`${root}#11`];
		const file = listing?.files.find((f) => f.relPath === 'src/switcher.ts');
		if (file) file.commentable = { left: [[1, 1]], right: [[1, 1]] };
		await installMockBridge(page, fixture);
		await openSwitcherDiff(page);

		await secondLine(page).click();
		await page.keyboard.press('Control+Alt+M');
		await expect(page.getByText("This line isn't part of the diff.")).toBeVisible();
		await expect(page.getByTestId('review-composer')).toHaveCount(0);
	});

	test('@smoke a draft survives a reload', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openSwitcherDiff(page);

		await secondLine(page).click();
		await page.keyboard.press('Control+Alt+M');
		await expect(page.getByTestId('review-composer-text')).toBeFocused();
		await page.keyboard.type('Still here after a reload');
		await page.keyboard.press('Control+Enter');
		await expect(page.getByTestId('review-draft')).toBeVisible();
		await expect
			.poll(() => page.evaluate((k) => localStorage.getItem(k) ?? '', DRAFTS_KEY))
			.toContain('Still here after a reload');

		await page.reload();
		// The panel remembers what was open; get to the pull request from either.
		await expect(page.getByTestId('file-tree-panel')).toBeVisible();
		await page.getByRole('tab', { name: 'PRs' }).click();
		const row = page.getByTestId('pr-row-11');
		await expect(row.or(page.getByTestId('pr-detail'))).toBeVisible();
		if (await row.isVisible()) await row.click();
		await expect(page.getByTestId('pr-review-tab-badge')).toHaveText('1');
		await page.getByTestId('pr-files').getByTitle('src/switcher.ts').click();
		await expect(page.getByTestId('review-draft')).toContainText('Still here after a reload');
	});

	test('@smoke cannot approve or request changes on your own pull request', async ({ page }) => {
		await installMockBridge(page, fixtureWithOwnPullRequest());
		await openReviewTab(page);

		await expect(page.getByTestId('pr-review-event-APPROVE')).toBeDisabled();
		await expect(page.getByTestId('pr-review-event-REQUEST_CHANGES')).toBeDisabled();
		await expect(page.getByTestId('pr-review-event-COMMENT')).toBeEnabled();
		await expect(page.getByTestId('pr-review-event-APPROVE')).toHaveAttribute(
			'title',
			/your own pull request/,
		);
	});

	test('@smoke publishes the review after a confirmation, and clears the drafts', async ({
		page,
	}) => {
		const fixture = fixtureWithPullRequests();
		const head = fixture.ghPrDetails?.[`${SLUG}#11`]?.headRefOid ?? '';
		await installMockBridge(page, fixture);
		await seedDraft(page, head);
		await openReviewTab(page);

		await expect(page.getByTestId('pr-review-tab-badge')).toHaveText('1');
		await page.getByTestId('pr-review-event-REQUEST_CHANGES').click();
		await page.getByTestId('pr-review-body').fill('Needs one rename.');
		await page.getByTestId('pr-review-submit').click();

		const confirm = page.getByTestId('pr-review-confirm');
		await expect(confirm).toContainText('@alice-reviewer');
		await expect(confirm.getByTestId('pr-review-confirm-text')).toHaveText(
			'Publish as @alice-reviewer: 1 comment, Request changes',
		);
		// Nothing has been sent yet.
		expect(await calls(page, 'gh_pr_submit_review')).toHaveLength(0);

		await confirm.getByTestId('pr-review-confirm-submit').click();
		await expect(page.getByText('Review published')).toBeVisible();

		const sent = (await calls(page, 'gh_pr_submit_review'))[0];
		expect(sent?.args?.number).toBe(11);
		const review = sent?.args?.review as {
			commitOid: string;
			event: string;
			body: string;
			comments: { path: string; line: number; side: string; body: string }[];
		};
		expect(review.commitOid).toBe(head);
		expect(review.event).toBe('REQUEST_CHANGES');
		expect(review.body).toBe('Needs one rename.');
		expect(review.comments).toHaveLength(1);
		expect(review.comments[0]).toMatchObject({
			path: 'src/switcher.ts',
			line: 1,
			side: 'RIGHT',
			body: 'Rename this constant.',
		});

		await expect(page.getByTestId('pr-review-tab-badge')).toHaveCount(0);
		await expect(page.getByTestId('pr-review-drafts')).toHaveCount(0);
		await expect(page.getByTestId('pr-review-body')).toHaveValue('');
	});

	test('@smoke keeps the drafts and says why when GitHub refuses the review', async ({ page }) => {
		const fixture = fixtureWithPullRequests();
		const head = fixture.ghPrDetails?.[`${SLUG}#11`]?.headRefOid ?? '';
		fixture.ghSubmitReviewFails = {
			kind: 'Remote',
			message: 'Unprocessable Entity: line must be part of the diff',
		};
		await installMockBridge(page, fixture);
		await seedDraft(page, head);
		await openReviewTab(page);

		await page.getByTestId('pr-review-submit').click();
		await page.getByTestId('pr-review-confirm-submit').click();

		await expect(page.getByTestId('pr-review-submit-error')).toContainText(
			'line must be part of the diff',
		);
		await expect(page.getByTestId('pr-review-tab-badge')).toHaveText('1');
		await expect(page.getByTestId('pr-review-draft')).toHaveCount(1);
		// One attempt: a failed submit is never retried behind the user's back.
		expect(await calls(page, 'gh_pr_submit_review')).toHaveLength(1);
	});

	test('@smoke warns when the pull request moved on under the drafts', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await seedDraft(page, 'a'.repeat(40));
		await openReviewTab(page);

		await expect(page.getByTestId('pr-review-stale')).toContainText('aaaaaaa');
		await page.getByRole('button', { name: 'Discard drafts' }).click();
		await expect(page.getByTestId('pr-review-stale')).toHaveCount(0);
		await expect(page.getByTestId('pr-review-tab-badge')).toHaveCount(0);
	});

	test('@smoke will not submit an empty comment review', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await openReviewTab(page);

		await expect(page.getByTestId('pr-review-submit')).toBeDisabled();
		await page.getByTestId('pr-review-event-APPROVE').click();
		await expect(page.getByTestId('pr-review-submit')).toBeEnabled();
	});
});
