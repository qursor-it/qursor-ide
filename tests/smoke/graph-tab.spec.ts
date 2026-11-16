import { expect, test } from '@playwright/test';
import {
	ALPHA_ID,
	FOO_ID,
	fixtureLongHistory,
	fixtureOneProjectOneSession,
	fixtureTwoProjectGraphs,
	fixtureWithGraph,
	installMockBridge,
	SHA_MERGE,
	ZULU_ID,
} from './fixtures';

/**
 * The panel's Graph tab (specs/05-features.md F18, F35): Git mode for the
 * session's checkout, in the panel. Lane assignment is tested in Rust; this
 * covers what the renderer decides: the tab mounts the action bar and the
 * table, folds its chips, docks a commit, follows the project, pages, and
 * switches to Local changes.
 */

const PROJECT = `/#/projects/${FOO_ID}`;

async function openGraph(page: import('@playwright/test').Page) {
	await page.getByRole('button', { name: 'Toggle file tree' }).click();
	await expect(page.getByTestId('file-tree-panel')).toBeVisible();
	await page.getByRole('tab', { name: 'Graph' }).click();
}

test.describe('graph tab', () => {
	test('@smoke the action bar, the table with folded chips, and a docked commit', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithGraph());
		await page.goto(PROJECT);
		await openGraph(page);

		const panel = page.getByTestId('git-panel');
		await expect(panel.getByTestId('git-action-bar')).toBeVisible();
		await expect(panel.getByRole('button', { name: 'Fetch' })).toBeVisible();
		await expect(page.getByTestId('commit-row')).toHaveCount(5);

		// HEAD on a branch in sync with its upstream: `origin/main` folds into it.
		const tip = page.getByTestId('commit-row').first();
		await expect(tip).toContainText('main');
		await expect(tip).not.toContainText('origin/main');
		await expect(tip.getByTestId('graph-rail')).toHaveCount(1);

		await page.locator(`[data-testid="commit-row"][data-sha="${SHA_MERGE}"]`).click();
		const detail = page.getByTestId('commit-detail');
		await expect(detail).toContainText('Titouan');
		await expect(detail).toContainText(SHA_MERGE);
		await expect(detail).toContainText('index.ts');
		await expect(page.getByRole('separator', { name: 'Resize commit detail' })).toBeVisible();
	});

	test('@smoke a project with no repository says so instead of erroring', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto(PROJECT);
		await openGraph(page);
		await expect(page.getByText('Not a git repository.')).toBeVisible();
	});

	test('@smoke switching project swaps the history and drops the selection', async ({ page }) => {
		await installMockBridge(page, fixtureTwoProjectGraphs());
		await page.goto(`/#/projects/${ZULU_ID}`);
		await openGraph(page);
		await expect(page.getByTestId('commit-row')).toHaveCount(3);
		await expect(page.getByText('aa commit 0')).toBeVisible();
		await page.getByTestId('commit-row').first().click();
		await expect(page.getByTestId('commit-detail')).toBeVisible();

		await page.goto(`/#/projects/${ALPHA_ID}`);
		await expect(page.getByTestId('commit-row')).toHaveCount(2);
		await expect(page.getByText('bb commit 0')).toBeVisible();
		await expect(page.getByTestId('commit-detail')).toHaveCount(0);
	});

	test('@smoke Load more appends the next page and then goes away', async ({ page }) => {
		await installMockBridge(page, fixtureLongHistory());
		await page.goto(PROJECT);
		await openGraph(page);
		await expect(page.getByTestId('commit-row')).toHaveCount(300);
		const loadMore = page.getByRole('button', { name: 'Load more' });
		await loadMore.click();
		await expect(page.getByTestId('commit-row')).toHaveCount(430);
		await expect(loadMore).toHaveCount(0);
	});

	test('@smoke Changes shows Local changes and Fetch runs from the panel', async ({ page }) => {
		await installMockBridge(page, fixtureWithGraph());
		await page.goto(PROJECT);
		await openGraph(page);
		await page.getByTestId('git-panel-changes').click();
		await expect(page.getByTestId('local-changes')).toBeVisible();
		await page.getByTestId('git-panel').getByRole('button', { name: 'Fetch' }).click();
		await expect
			.poll(() =>
				page.evaluate(() =>
					(window.__FACTORAI_TEST_CALLS__ ?? [])
						.filter((c) => c.name === 'git_op')
						.map((c) => (c.args as { op: { kind: string } }).op.kind),
				),
			)
			.toEqual(['fetch']);
		await expect(page.getByTestId('git-mode-view')).toHaveCount(0);
	});
});
