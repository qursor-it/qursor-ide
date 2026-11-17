import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureOneProjectOneSession, installMockBridge } from './fixtures';

/**
 * Canvas mode (specs/05-features.md F44): the third rail entry, a canvas list per
 * project, the create dialog and the read-only board.
 *
 * The browser bridge has no disk: `create_canvas` writes into the fixture's
 * `files`, and the Rust side (slug, `create_new`, folder) has its own tests.
 */
test.describe('canvas mode', () => {
	test('@smoke creating a canvas lists it and opens its board', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('mode-canvas').click();
		await expect(page.getByTestId('canvas-mode-view')).toBeVisible();

		await page.getByTestId('new-canvas').click();
		await page.getByLabel('Canvas title').fill('Brainstorm MVP');
		await page.getByRole('button', { name: 'Create' }).click();

		await expect(page.getByTestId('canvas-row')).toHaveText('Brainstorm MVP');
		await expect(page.getByTestId('canvas-board')).toBeVisible();

		const saved = await page.evaluate(() =>
			Object.entries(window.__FACTORAI_TEST__?.files ?? {}).find(([p]) => p.endsWith('.canvas')),
		);
		expect(saved?.[0]).toBe('/home/alice/code/foo/canvas/brainstorm-mvp.canvas');
		expect(saved?.[1].contents).toContain('"title": "Brainstorm MVP"');
	});

	test('@smoke an unreadable canvas shows an error card, not a board', async ({ page }) => {
		const fx = fixtureOneProjectOneSession();
		const path = '/home/alice/code/foo/canvas/broken.canvas';
		fx.canvasesByProject = {
			[FOO_ID]: [
				{
					projectId: FOO_ID,
					path,
					relPath: 'canvas/broken.canvas',
					slug: 'broken',
					title: 'broken',
					modifiedMs: 1,
				},
			],
		};
		fx.files = { ...fx.files, [path]: { path, contents: '{', isBinary: false } as never };
		await installMockBridge(page, fx);
		await page.goto('/');
		await page.getByTestId('mode-canvas').click();
		await page.getByTestId('canvas-row').click();
		await expect(page.getByTestId('canvas-error')).toBeVisible();
		await expect(page.getByRole('button', { name: 'Open as text' })).toBeVisible();
		await expect(page.getByTestId('canvas-board')).toHaveCount(0);
	});
});
