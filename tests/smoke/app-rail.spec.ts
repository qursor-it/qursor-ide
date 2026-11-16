/**
 * ADR-0093, ADR-0100 — the app rail: the two modes on the left edge, always
 * drawn, and the project tree beside it.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureOneProjectOneSession, installMockBridge } from './fixtures';

test.describe('app rail', () => {
	test('@smoke collapsing hides the column and keeps the rail; Sessions brings it back', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('sidebar-collapse').click();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);
		await expect(page.getByTestId('app-rail')).toBeVisible();

		await page.reload();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);

		await page.getByTestId('mode-sessions').click();
		await expect(page.getByTestId('projects')).toBeVisible();
	});

	test('@smoke has no Inbox, Board or project tiles, and stays in Git mode', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto(`/#/projects/${FOO_ID}`);
		await expect(page.getByTestId('app-rail').getByRole('button', { name: 'foo' })).toHaveCount(0);
		await expect(page.getByTestId('inbox')).toHaveCount(0);
		await expect(page.getByTestId('open-board')).toHaveCount(0);

		await page.getByRole('tab', { name: 'Git' }).click();
		await expect(page.getByTestId('app-rail')).toBeVisible();
		await page.getByRole('tab', { name: 'Sessions' }).click();
		await expect(page.getByTestId('git-mode-view')).toHaveCount(0);
	});

	test('@smoke the Sessions badge counts the sessions that need you', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.locator('aside nav').getByText('foo').click();
		await page.getByText('Refactor the auth middleware').click();
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await expect(page.getByTestId('mode-sessions-count')).toHaveCount(0);

		await page.evaluate(() =>
			window.__FACTORAI_EMIT__?.('terminal:status', {
				id: 'mock-terminal-id',
				status: 'waiting_input',
				lastActivity: Date.now(),
			}),
		);
		await expect(page.getByTestId('mode-sessions-count')).toHaveText('1');
	});
});
