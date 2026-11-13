/**
 * F39 — answering the agent from outside its terminal: the waiting banner, its
 * permission-menu buttons, and the reply box. Everything they send leaves
 * through `terminal_write`, the same command typing uses.
 */
import { expect, test, type Page } from '@playwright/test';
import { fixtureOneProjectOneSession, installMockBridge } from './fixtures';

const TERMINAL_ID = 'mock-terminal-id';

async function open(page: Page) {
	await installMockBridge(page, fixtureOneProjectOneSession());
	await page.goto('/');
	await page.locator('aside').getByText('foo').click();
	await page.getByText('Refactor the auth middleware').click();
	await expect(page.locator('.xterm:visible')).toBeVisible();
}

const write = (page: Page, text: string) =>
	page.evaluate(
		({ id, text }) =>
			window.__FACTORAI_EMIT__?.('terminal:data', {
				id,
				bytesB64: btoa(unescape(encodeURIComponent(text))),
			}),
		{ id: TERMINAL_ID, text: text.replace(/\n/g, '\r\n') },
	);
const status = (page: Page, s: string) =>
	page.evaluate(
		({ id, s }) =>
			window.__FACTORAI_EMIT__?.('terminal:status', { id, status: s, lastActivity: Date.now() }),
		{ id: TERMINAL_ID, s },
	);
const written = (page: Page) =>
	page.evaluate(() =>
		(window.__FACTORAI_TEST_CALLS__ ?? [])
			.filter((c) => c.name === 'terminal_write')
			.map((c) => (c.args as { data: string }).data)
			.join(''),
	);

test.describe('agent reply', () => {
	test('@smoke a permission menu becomes buttons that press its numbers', async ({ page }) => {
		await open(page);
		await expect(page.getByTestId('waiting-banner')).toHaveCount(0);
		await write(
			page,
			'● Edit docs/install.md\n\n Do you want to make this edit to install.md?\n ❯ 1. Yes\n   2. Yes, allow all edits during this session\n   3. No, and tell Claude what to do differently (esc)\n',
		);
		await status(page, 'waiting_input');

		const banner = page.getByTestId('waiting-banner');
		await expect(banner).toContainText('Do you want to make this edit to install.md?');
		await expect(banner.getByTestId('waiting-option-3')).toContainText('No, and tell Claude');
		await banner.getByTestId('waiting-option-1').click();
		await expect.poll(() => written(page)).toContain('1');

		await status(page, 'working');
		await expect(banner).toHaveCount(0);
	});

	test('@smoke an ordinary wait offers the terminal, not guessed keys', async ({ page }) => {
		await open(page);
		await write(page, 'Which port should Caddy expose?\n');
		await status(page, 'waiting_input');
		const banner = page.getByTestId('waiting-banner');
		await expect(banner).toContainText('is waiting for you');
		await expect(banner.locator('[data-testid^="waiting-option-"]')).toHaveCount(0);
	});

	test('@smoke the reply box sends the text and an Enter', async ({ page }) => {
		await open(page);
		const box = page.getByTestId('reply-box');
		await box.fill('Use port 8443');
		await box.press('Enter');
		await expect(box).toHaveValue('');
		await expect.poll(() => written(page)).toContain('Use port 8443');
		await expect.poll(() => written(page)).toMatch(/Use port 8443(\x1b\[201~)?\r/);
	});
});
