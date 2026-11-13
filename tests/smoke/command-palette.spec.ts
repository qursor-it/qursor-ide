/**
 * F37 — the command palette: one field for sessions, projects and actions.
 */
import { expect, test } from '@playwright/test';
import { fixtureWithSearchHits, installMockBridge } from './fixtures';

test.describe('command palette', () => {
	test('@smoke Mod+K opens it, typing finds a session, Enter opens it', async ({ page }) => {
		await installMockBridge(page, fixtureWithSearchHits());
		await page.goto('/');
		await page.keyboard.press('ControlOrMeta+k');
		const palette = page.getByTestId('command-palette');
		await expect(palette).toBeVisible();

		await palette.getByRole('combobox').fill('auth');
		const hit = palette.getByTestId('palette-item-s:session-uuid-001');
		await expect(hit).toContainText('Refactor the auth middleware');
		await expect(hit).toHaveAttribute('aria-selected', 'true');
		await page.keyboard.press('Enter');

		await expect(palette).toHaveCount(0);
		await expect(page).toHaveURL(/sessions\/session-uuid-001/);
		await expect(page.locator('.xterm:visible')).toBeVisible();
	});

	test('@smoke the top bar opens it, a project matches by name, Esc closes', async ({ page }) => {
		await installMockBridge(page, fixtureWithSearchHits());
		await page.goto('/');
		await page.getByTestId('open-palette').click();
		const palette = page.getByTestId('command-palette');
		await palette.getByRole('combobox').fill('fo');
		await expect(palette.getByRole('option', { name: /foo/ }).first()).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(palette).toHaveCount(0);
	});

	test('@smoke an action runs from it', async ({ page }) => {
		await installMockBridge(page, fixtureWithSearchHits());
		await page.goto('/');
		await page.getByTestId('open-palette').click();
		await page.getByTestId('command-palette').getByRole('combobox').fill('settings');
		await page.keyboard.press('Enter');
		await expect(page.getByTestId('settings-modal')).toBeVisible();
	});
});
