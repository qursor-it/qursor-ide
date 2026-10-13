import { expect, test, type Page } from '@playwright/test';
import {
	FOO_ID,
	fixtureWithFileTree,
	fixtureWithPullRequests,
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

	test('@smoke lists the pull requests and marks only what earns a mark', async ({ page }) => {
		await installMockBridge(page, fixtureWithPullRequests());
		await page.goto(PROJECT);
		await openPrs(page);

		// Open is the default filter: #13 is merged and is not listed.
		await expect(page.getByTestId('pr-row-11')).toContainText('feat: add the widget switcher');
		await expect(page.getByTestId('pr-row-12')).toContainText('wip: rewrite the renderer');
		await expect(page.getByTestId('pr-row-13')).toHaveCount(0);
		await expect(page.getByTestId('pr-repo')).toHaveText('acme/widgets');

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
