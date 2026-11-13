/**
 * F36 — the Inbox: the sessions that want you, then the ones working, above the
 * project tree.
 */
import { expect, test, type Page } from '@playwright/test';
import { FOO_ID, fixtureOneProjectOneSession, installMockBridge, showInbox } from './fixtures';

const TERMINAL_ID = 'mock-terminal-id';
const SESSION_ID = 'session-uuid-001';

async function emitStatus(page: Page, status: 'working' | 'waiting_input' | 'stopped') {
	await page.evaluate(
		({ id, status }) =>
			window.__FACTORAI_EMIT__?.('terminal:status', { id, status, lastActivity: Date.now() }),
		{ id: TERMINAL_ID, status },
	);
}

test.describe('inbox', () => {
	test('@smoke is absent until a session runs, then follows its status', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await expect(page.getByTestId('inbox')).toHaveCount(0);

		await page.locator('aside').getByText('foo').click();
		await page.getByText('Refactor the auth middleware').click();
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await showInbox(page);

		const row = page.getByTestId(`inbox-row-${SESSION_ID}`);
		await expect(
			page.getByTestId('inbox-working').getByTestId(`inbox-row-${SESSION_ID}`),
		).toBeVisible();
		await expect(row).toContainText('Refactor the auth middleware');
		await expect(row).toContainText('foo');
		await expect(row).toHaveAttribute('aria-current', 'page');

		await emitStatus(page, 'waiting_input');
		await expect(
			page.getByTestId('inbox-needs-you').getByTestId(`inbox-row-${SESSION_ID}`),
		).toBeVisible();
		await expect(page.getByTestId('inbox-working')).toHaveCount(0);

		// Stopped while you are looking at it: nothing to see, but still open, so
		// it moves to Open until you close it (F16).
		await emitStatus(page, 'stopped');
		await expect(
			page.getByTestId('inbox-open').getByTestId(`inbox-row-${SESSION_ID}`),
		).toBeVisible();
		await expect(page.getByTestId('inbox-needs-you')).toHaveCount(0);
	});

	test('@smoke a row opens its session', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.locator('aside').getByText('foo').click();
		await page.getByText('Refactor the auth middleware').click();
		await expect(page.locator('.xterm:visible')).toBeVisible();

		await showInbox(page);
		await page.getByTestId(`rail-project-${FOO_ID}`).click();
		await expect(page).not.toHaveURL(/sessions/);
		await page.getByTestId(`inbox-row-${SESSION_ID}`).click();
		await expect(page).toHaveURL(new RegExp(`sessions/${SESSION_ID}`));
	});
});
