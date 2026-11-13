/**
 * ADR-0093 — the app rail: places on the left edge, always drawn, and the
 * column beside it that shows the Inbox or the project tree.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureOneProjectOneSession, installMockBridge } from './fixtures';

test.describe('app rail', () => {
	test('@smoke switches the column between the project tree and the Inbox, and remembers it', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await expect(page.getByTestId('projects')).toBeVisible();

		await page.getByTestId('mode-sessions').click();
		await expect(page.getByTestId('inbox-empty')).toBeVisible();
		await expect(page.getByTestId('projects')).toHaveCount(0);

		await page.reload();
		await expect(page.getByTestId('inbox-empty')).toBeVisible();

		await page.getByTestId('inbox-empty').getByRole('button', { name: 'Browse projects' }).click();
		await expect(page.getByTestId('projects')).toBeVisible();
	});

	test('@smoke collapsing hides the column and keeps the rail; a rail item brings it back', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('sidebar-collapse').click();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);
		await expect(page.getByTestId('app-rail')).toBeVisible();

		await page.reload();
		await expect(page.getByTestId('sidebar')).toHaveCount(0);

		await page.getByTestId('rail-projects').click();
		await expect(page.getByTestId('projects')).toBeVisible();
	});

	test('@smoke a project tile opens its page, and the rail stays in Git mode', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId(`rail-project-${FOO_ID}`).click();
		await expect(page).toHaveURL(new RegExp(`projects/${FOO_ID}$`));

		await page.getByRole('tab', { name: 'Git' }).click();
		await expect(page.getByTestId('app-rail')).toBeVisible();
		await page.getByRole('tab', { name: 'Sessions' }).click();
		await expect(page.getByTestId('git-mode-view')).toHaveCount(0);
	});

	test('@smoke the Inbox badge counts the sessions that need you', async ({ page }) => {
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
