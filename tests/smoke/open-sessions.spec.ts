import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import {
	FOO_ID,
	ZULU_ID,
	fixtureOneProjectOneSession,
	fixtureTwoProjectsManySessions,
	installMockBridge,
	showInbox,
} from './fixtures';

/**
 * What you have open, in the sidebar's Inbox (specs/05-features.md F16, F36,
 * ADR-0092): it took over from the top bar's tab strip, so these are that
 * strip's rules — a row per open session, the close × and its confirm, the
 * keyboard, and sessions restored stopped on launch.
 */
/** Open a session from the project tree, then look at the Inbox (ADR-0093). */
async function openSession(page: Page, name: RegExp) {
	await page.getByTestId('rail-projects').click();
	await page.getByRole('link', { name }).click();
	await expect(page.locator('.xterm:visible')).toBeVisible();
	await showInbox(page);
}

const row = (page: Page, sessionId: string) => page.getByTestId(`inbox-row-${sessionId}`);

test.describe('open sessions', () => {
	test('@smoke there is no tab strip, and the list appears with the first session', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureTwoProjectsManySessions());
		await page.goto('/');
		await expect(page.getByTestId('session-tabs')).toHaveCount(0);
		await expect(page.getByTestId('inbox')).toHaveCount(0);

		await page.getByRole('button', { name: 'Expand zulu' }).click();
		await openSession(page, /Zulu task 11/);
		await expect(row(page, 'zulu-session-11')).toBeVisible();
		await expect(page.getByTestId('session-tabs')).toHaveCount(0);
	});

	test('@smoke one row per open session, the current one marked, a click switches', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureTwoProjectsManySessions());
		await page.goto('/');
		await page.getByRole('button', { name: 'Expand zulu' }).click();
		await openSession(page, /Zulu task 11/);
		await openSession(page, /Zulu task 10/);

		await expect(row(page, 'zulu-session-10')).toHaveAttribute('aria-current', 'page');
		await expect(row(page, 'zulu-session-11')).not.toHaveAttribute('aria-current', 'page');

		await row(page, 'zulu-session-11').click();
		await expect(page).toHaveURL(/sessions\/zulu-session-11$/);
		await expect(row(page, 'zulu-session-11')).toHaveAttribute('aria-current', 'page');
	});

	test('@smoke closing asks first, and kills the PTY on confirm', async ({ page }) => {
		await installMockBridge(page, fixtureTwoProjectsManySessions());
		await page.goto('/');
		await page.getByRole('button', { name: 'Expand zulu' }).click();
		await openSession(page, /Zulu task 11/);

		await page.getByTestId('inbox-row-zulu-session-11-close').click();
		await expect(page.getByText('Close this session?')).toBeVisible();
		let calls = await page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
		expect(calls.some((c) => c.name === 'terminal_kill')).toBe(false);

		await page.getByRole('button', { name: /Close & kill session/ }).click();
		calls = await page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
		expect(calls.some((c) => c.name === 'terminal_kill')).toBe(true);
		await expect(page.getByTestId('inbox')).toHaveCount(0);
		await expect(page).toHaveURL(new RegExp(`projects/${ZULU_ID}$`));
	});

	test('@smoke keeping it running closes the dialog and changes nothing', async ({ page }) => {
		await installMockBridge(page, fixtureTwoProjectsManySessions());
		await page.goto('/');
		await page.getByRole('button', { name: 'Expand zulu' }).click();
		await openSession(page, /Zulu task 11/);

		await page.getByTestId('inbox-row-zulu-session-11-close').click();
		await page.getByRole('button', { name: /Keep it running/ }).click();
		await expect(page.getByText('Close this session?')).toHaveCount(0);
		await expect(row(page, 'zulu-session-11')).toBeVisible();
		const calls = await page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
		expect(calls.some((c) => c.name === 'terminal_kill')).toBe(false);
	});

	test('@smoke middle-clicking a row asks before closing it', async ({ page }) => {
		await installMockBridge(page, fixtureTwoProjectsManySessions());
		await page.goto('/');
		await page.getByRole('button', { name: 'Expand zulu' }).click();
		await openSession(page, /Zulu task 11/);

		await row(page, 'zulu-session-11').click({ button: 'middle' });
		await expect(page.getByText('Close this session?')).toBeVisible();
		await page.getByRole('button', { name: /Close & kill session/ }).click();
		await expect(page.getByTestId('inbox')).toHaveCount(0);
	});

	test('@smoke Mod+PageDown steps to the next open session, Mod+W closes it', async ({ page }) => {
		await installMockBridge(page, fixtureTwoProjectsManySessions());
		await page.goto('/');
		await page.getByRole('button', { name: 'Expand zulu' }).click();
		await openSession(page, /Zulu task 11/);
		await openSession(page, /Zulu task 10/);
		await page.locator('main header').click();

		await page.keyboard.press('ControlOrMeta+PageDown');
		await expect(page).toHaveURL(/sessions\/zulu-session-11$/);

		await page.keyboard.press('ControlOrMeta+w');
		await expect(page.getByText('Close this session?')).toBeVisible();
	});
});

test.describe('restored sessions', () => {
	/** Seed the persisted list the way a previous run would have left it (F16),
	 *  in Zustand's `persist` envelope — the bytes the store itself writes. */
	async function seedTabs(page: Page, tabs: Array<{ sessionId: string; projectId: string }>) {
		await page.addInitScript((t) => {
			window.localStorage.setItem(
				'factorai.terminals',
				JSON.stringify({ state: { tabs: t }, version: 1 }),
			);
		}, tabs);
	}

	test('@smoke come back stopped on launch, and spawn when clicked', async ({ page }) => {
		await seedTabs(page, [{ sessionId: 'session-uuid-001', projectId: FOO_ID }]);
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await showInbox(page);

		const open = page.getByTestId('inbox-open');
		const r = open.getByTestId('inbox-row-session-uuid-001');
		await expect(r).toContainText('Refactor the auth middleware');
		await expect(r.locator('[title="Stopped"]')).toHaveCount(1);
		// Nothing spawned until the click.
		await expect(page.locator('.xterm')).toHaveCount(0);

		await r.click();
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await expect(
			page.getByTestId('inbox-working').getByTestId('inbox-row-session-uuid-001'),
		).toBeVisible();
	});

	test('@smoke a stopped open session closes without asking', async ({ page }) => {
		await seedTabs(page, [{ sessionId: 'session-uuid-001', projectId: FOO_ID }]);
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await showInbox(page);
		await row(page, 'session-uuid-001').hover();
		await page.getByTestId('inbox-row-session-uuid-001-close').click();
		await expect(page.getByText('Close this session?')).toHaveCount(0);
		await expect(page.getByTestId('inbox')).toHaveCount(0);
	});
});
