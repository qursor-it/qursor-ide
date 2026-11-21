/**
 * The frame every mode shares: one full-height sidebar with the top bar beside
 * it, the Sessions body inside it kept mounted across modes.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureOneProjectOneSession, installMockBridge } from './fixtures';

async function openSession(page: import('@playwright/test').Page) {
	await page.getByTestId('sidebar').getByText('foo').click();
	await page.getByText('Refactor the auth middleware').click();
}

test.describe('unified sidebar', () => {
	test('@smoke the sidebar runs from the top and the top bar starts at its edge', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await openSession(page);

		const sidebar = (await page.getByTestId('sidebar').boundingBox())!;
		const palette = (await page.getByTestId('open-palette').boundingBox())!;
		const chip = (await page.getByTestId('tab-group-chip').first().boundingBox())!;
		expect(sidebar.y).toBeLessThan(palette.y);
		expect(palette.x).toBeGreaterThan(sidebar.x + sidebar.width);
		expect(chip.x).toBeGreaterThanOrEqual(sidebar.x + sidebar.width);
		await expect(page.getByTestId('sidebar').getByText('Qursor')).toBeVisible();
	});

	test('@smoke a shell survives a mode round trip', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await openSession(page);
		await page.getByTestId('shell-footer').getByRole('button', { name: 'Terminal' }).click();
		await expect(page.getByTestId('shell-pane-host')).toHaveCount(1);

		await page.getByTestId('mode-git').click();
		await page.getByTestId('mode-sessions').click();

		await expect(page.getByTestId('shell-pane-host')).toHaveCount(1);
		const calls = await page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
		expect(calls.filter((c) => c.name === 'shell_spawn')).toHaveLength(1);
	});

	test('@smoke the sidebar search survives a mode round trip', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		const search = page.getByPlaceholder('Search projects and sessions…');
		await search.fill('foo');

		await page.getByTestId('mode-git').click();
		await page.getByTestId('mode-sessions').click();

		await expect(search).toHaveValue('foo');
	});

	test('@smoke at 1100px with a 480px sidebar the search and tabs are on screen', async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1100, height: 800 });
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await openSession(page);

		const handle = (await page.getByRole('separator', { name: 'Resize sidebar' }).boundingBox())!;
		const y = handle.y + handle.height / 2;
		await page.mouse.move(handle.x + handle.width / 2, y);
		await page.mouse.down();
		await page.mouse.move(handle.x + 900, y, { steps: 5 });
		await page.mouse.up();
		expect((await page.getByTestId('sidebar').boundingBox())!.width).toBe(480);

		for (const id of ['open-palette', 'tab-group-chip']) {
			const box = (await page.getByTestId(id).first().boundingBox())!;
			expect(box.x + box.width).toBeLessThanOrEqual(1100);
		}
	});

	test('@smoke the file tree can still widen at 1100px with the default sidebar', async ({
		page,
	}) => {
		// At 480px the ceiling is under the panel's floor by arithmetic; the default
		// width is where a stale sidebar term in the ceiling would show.
		await page.setViewportSize({ width: 1100, height: 800 });
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await openSession(page);

		await page.getByRole('button', { name: 'Toggle file tree' }).click();
		const panel = page.getByTestId('file-tree-panel');
		await expect(panel).toBeVisible();
		const before = (await panel.boundingBox())!.width;
		const handle = page.getByRole('separator', { name: 'Resize file tree' });
		await handle.focus();
		await handle.press('ArrowLeft');
		await handle.press('ArrowLeft');
		expect((await panel.boundingBox())!.width).toBeGreaterThan(before);
	});

	test('@smoke focus follows the toggle in both directions', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		const sidebar = page.getByTestId('sidebar');
		await sidebar.getByRole('button', { name: 'Collapse sidebar' }).click();
		await expect(page.getByRole('button', { name: 'Expand sidebar' })).toBeFocused();

		await page.getByRole('button', { name: 'Expand sidebar' }).click();
		await expect(sidebar.getByRole('button', { name: 'Collapse sidebar' })).toBeFocused();
	});

	test('@smoke canvas lists its canvases in the shared sidebar', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('mode-canvas').click();

		await expect(page.getByTestId('sidebar').getByTestId('canvas-sidebar')).toBeVisible();
		await expect(page.getByRole('separator', { name: 'Resize sidebar' })).toHaveCount(1);
	});

	test("@smoke git's tree is in the shared sidebar", async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto(`/#/projects/${FOO_ID}`);
		await page.getByTestId('mode-git').click();

		await expect(page.getByTestId('sidebar').getByTestId('git-sidebar')).toBeVisible();
	});

	test('@smoke collapse in Git mode, expand from the top bar', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto(`/#/projects/${FOO_ID}`);
		await page.getByTestId('mode-git').click();
		await expect(page.getByTestId('git-sidebar')).toBeVisible();

		await page.getByTestId('sidebar-collapse').click();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);
		await expect(page.getByTestId('git-sidebar')).toHaveCount(0);
		await page.getByRole('button', { name: 'Expand sidebar' }).click();

		await expect(page.getByTestId('git-sidebar')).toBeVisible();
		await expect(page.getByTestId('git-mode-view')).toBeVisible();
	});

	test('@smoke servers: the book and the vault are in the shared sidebar', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		const sessionsWidth = (await page.getByTestId('sidebar').boundingBox())!.width;
		await page.getByTestId('mode-servers').click();

		const sidebar = page.getByTestId('sidebar');
		await expect(sidebar.getByTestId('server-book')).toBeVisible();
		await expect(sidebar.getByTestId('sidebar-footer').getByTestId('vault-state')).toBeVisible();
		expect((await sidebar.boundingBox())!.width).toBe(sessionsWidth);
	});

	test('@smoke the sidebar footer and the shell footer share a bottom edge', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await openSession(page);
		await expect(page.getByTestId('shell-footer')).toBeVisible();

		const side = (await page.getByTestId('sidebar-footer').boundingBox())!;
		const shell = (await page.getByTestId('shell-footer').boundingBox())!;
		expect(Math.abs(side.y + side.height - (shell.y + shell.height))).toBeLessThanOrEqual(1);
	});

	test('@smoke a collapsed sidebar expands on the icon of the mode you are in only', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('sidebar-collapse').click();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);

		await page.getByTestId('mode-canvas').click();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);
		await page.getByTestId('mode-canvas').click();
		await expect(page.getByTestId('sidebar')).toBeVisible();
	});
});
