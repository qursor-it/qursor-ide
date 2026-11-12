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

	test('@smoke a missing git is shown in the op log', async ({ page }) => {
		await installMockBridge(
			page,
			fixtureWithRefs({ gitOpFails: { kind: 'NotFound', message: 'git not found on PATH' } }),
		);
		await page.goto(PROJECT);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByRole('button', { name: 'Fetch' }).click();
		await expect(page.getByTestId('op-log')).toContainText('git not found on PATH');
		// History still works.
		await expect(page.getByTestId('commit-row')).toHaveCount(5);
	});
});
