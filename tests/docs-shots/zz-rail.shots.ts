import { expect, test, type Page } from '@playwright/test';
import { installMockBridge } from '../smoke/fixtures';
import { agentWrites } from './capture';
import { IDS, world } from './world';
const OUT =
	'/tmp/claude-0/-home-user-qursor-ide/77ea294b-06b2-5a9a-921b-0203f583edf2/scratchpad/final';
const st = (page: Page, id: string, s: string) =>
	page.evaluate(
		({ id, s }) =>
			window.__FACTORAI_EMIT__?.('terminal:status', { id, status: s, lastActivity: Date.now() }),
		{ id, s },
	);
for (const scheme of ['light', 'dark'] as const) {
	test(scheme, async ({ page }) => {
		await page.emulateMedia({ colorScheme: scheme });
		const fx = world();
		await installMockBridge(page, fx);
		await page.goto('/');
		await page.screenshot({ path: `${OUT}/r0-${scheme}-empty.png` });
		await page.getByTestId('rail-projects').click();
		const open = async (title: string, id: string) => {
			await page.evaluate((i) => {
				if (window.__FACTORAI_TEST__) window.__FACTORAI_TEST__.terminalSpawnId = i;
			}, id);
			await page
				.getByRole('link', { name: new RegExp(title) })
				.first()
				.click();
			await expect(page.locator('.xterm:visible')).toBeVisible();
		};
		for (const name of ['Pro', 'Side projects'])
			await page
				.getByRole('button', { name: `Expand ${name}`, exact: true })
				.click()
				.catch(() => {});
		for (const p of ['billing-api', 'docs-site', 'homelab'])
			await page
				.getByRole('button', { name: `Expand ${p}`, exact: true })
				.click()
				.catch(() => {});
		await open('Move Jellyfin', 'pty-j');
		await open('Write the install page', 'pty-i');
		await open('Stripe retries', 'pty-s');
		for (const id of ['pty-j', 'pty-i', 'pty-s']) await st(page, id, 'working');
		await st(page, 'pty-j', 'waiting_input');
		await page.getByRole('tab', { name: 'Sessions' }).click();
		await agentWrites(page, 'pty-s', '\x1b[1m> Stripe retries are duplicating invoices.\x1b[0m\n');
		await page.mouse.move(-10, -10);
		await page.waitForTimeout(500);
		await page.screenshot({ path: `${OUT}/r1-${scheme}-inbox.png` });
	});
}
