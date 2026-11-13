/**
 * F38 — the Board: every open or live session in a column per state.
 */
import { expect, test, type Page } from '@playwright/test';
import { fixtureOneProjectOneSession, installMockBridge } from './fixtures';

const SESSION_ID = 'session-uuid-001';
const emitStatus = (page: Page, status: string) =>
	page.evaluate(
		(status) =>
			window.__FACTORAI_EMIT__?.('terminal:status', {
				id: 'mock-terminal-id',
				status,
				lastActivity: Date.now(),
			}),
		status,
	);

test.describe('board', () => {
	test('@smoke says so when nothing is open', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('open-board').click();
		await expect(page).toHaveURL(/#\/board/);
		await expect(page.getByText('No session is open or running')).toBeVisible();
	});

	test('@smoke moves a session between columns and opens it', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.locator('aside').getByText('foo').click();
		await page.getByText('Refactor the auth middleware').click();
		await expect(page.locator('.xterm:visible')).toBeVisible();

		await page.getByTestId('open-board').click();
		const card = page.getByTestId(`board-card-${SESSION_ID}`);
		await expect(
			page.getByTestId('board-working').getByTestId(`board-card-${SESSION_ID}`),
		).toBeVisible();
		await expect(card).toContainText('Refactor the auth middleware');

		await emitStatus(page, 'waiting_input');
		await expect(
			page.getByTestId('board-needs-you').getByTestId(`board-card-${SESSION_ID}`),
		).toBeVisible();

		await card.click();
		await expect(page).toHaveURL(new RegExp(`sessions/${SESSION_ID}`));
	});
});
