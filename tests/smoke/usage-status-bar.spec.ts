/**
 * Usage monitor — the status bar along the window's bottom edge.
 *
 * Rust owns the readings (`get_usage`, `usage:changed`, `session_usage`);
 * these tests own what the renderer draws from them: one group per installed
 * agent with its default profile, a fill that is neutral until a threshold,
 * the popover listing every profile, and the pref that hides the bar.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureWithUsage, installMockBridge } from './fixtures';

test.describe('usage status bar', () => {
	test('@smoke usage status bar shows limits, colours the one past 90%, opens the detail', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithUsage());
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);

		const bar = page.getByTestId('status-bar');
		await expect(bar).toBeVisible();
		await expect(page.getByTestId('usage-group-claude')).toContainText('62%');
		await expect(page.getByTestId('usage-group-claude')).toContainText('31%');
		await expect(page.getByTestId('usage-group-codex')).toContainText('92%');

		// Neutral below 70, red from 90.
		await expect(page.getByTestId('usage-fill-claude-5h')).toHaveClass(/bg-muted-foreground/);
		await expect(page.getByTestId('usage-fill-codex-5h')).toHaveClass(/bg-destructive/);

		// The session in front, on the right.
		await expect(page.getByTestId('status-bar-session')).toHaveText('1.8M tok · ~$4.12');

		await bar.click();
		const popover = page.getByTestId('usage-popover');
		await expect(popover).toBeVisible();
		await expect(popover.getByText('Weekly').first()).toBeVisible();
		// Every profile, not only the default the bar shows.
		await expect(popover.getByText('Work')).toBeVisible();
		await expect(popover.getByText('This session')).toBeVisible();
	});

	test('@smoke usage status bar does not overflow at the minimum window width', async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1100, height: 700 });
		await installMockBridge(page, fixtureWithUsage());
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);

		const bar = page.getByTestId('status-bar');
		await expect(bar).toBeVisible();
		await expect(page.getByTestId('usage-group-claude')).toContainText('62%');
		const fits = await bar.evaluate((el) => el.scrollWidth <= el.clientWidth);
		expect(fits).toBe(true);
	});

	test('@smoke usage status bar is absent when turned off', async ({ page }) => {
		await page.addInitScript(() => {
			window.localStorage.setItem(
				'factorai.prefs',
				JSON.stringify({ state: { showStatusBar: false }, version: 1 }),
			);
		});
		await installMockBridge(page, fixtureWithUsage());
		await page.goto('/');

		await expect(page.getByTestId('sidebar')).toBeVisible();
		await expect(page.getByTestId('status-bar')).toHaveCount(0);
	});
});
