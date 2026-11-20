/**
 * Usage thresholds and the Windows taskbar widget's settings.
 *
 * The widget itself is native and cannot be driven here; these tests own what
 * the renderer does around it: the form that edits its layout, the global
 * thresholds that recolour the status bar, and the click that opens the
 * details.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureWithUsage, installMockBridge } from './fixtures';

test.describe('taskbar widget settings', () => {
	// A Windows user agent: the widget's section is Windows-only.
	test.use({
		userAgent:
			'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML) Chrome/130.0 Safari/537.36',
	});

	test('@smoke saving a widget option and the thresholds writes both', async ({ page }) => {
		await installMockBridge(page, fixtureWithUsage());
		await page.goto('/?settings=usage');

		await expect(page.getByTestId('settings-widget')).toBeVisible();
		await page.getByTestId('settings-widget-layout').click();
		await page.getByTestId('settings-widget-layout-inline').click();
		await page.getByTestId('settings-widget-show-weeklyBar').click();
		await page.getByTestId('settings-usage-warn').fill('50');
		await page.getByTestId('settings-usage-critical').fill('80');
		await expect(page.getByTestId('settings-dirty-usage')).toBeVisible();
		await page.getByTestId('settings-save').click();
		await expect(page.getByTestId('settings-modal')).toHaveCount(0);

		const calls = await page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
		const sets = calls.filter((c) => c.name === 'set_setting').map((c) => c.args);
		expect(sets).toContainEqual({ key: 'usageWarnPercent', value: '50' });
		expect(sets).toContainEqual({ key: 'usageCriticalPercent', value: '80' });
		const widget = calls.filter((c) => c.name === 'set_taskbar_widget_config');
		expect(widget).toHaveLength(1);
		const config = (widget[0].args as { config: Record<string, unknown> }).config;
		expect(config).toMatchObject({
			layout: 'inline',
			gap: 8,
			width: 244,
			show: { weeklyBar: true, fiveHourBar: true },
		});
	});

	test('@smoke thresholds are validated before they can be saved', async ({ page }) => {
		await installMockBridge(page, fixtureWithUsage());
		await page.goto('/?settings=usage');

		await page.getByTestId('settings-usage-warn').fill('95');
		await expect(page.getByTestId('settings-usage-thresholds-error')).toContainText(
			'below critical',
		);
		await expect(page.getByTestId('settings-save')).toBeDisabled();
		await page.getByTestId('settings-usage-critical').fill('99');
		await expect(page.getByTestId('settings-usage-thresholds-error')).toHaveCount(0);
		await expect(page.getByTestId('settings-save')).toBeEnabled();
	});

	test('@smoke the agents are reordered and reset restores the position and size', async ({
		page,
	}) => {
		const fixture = fixtureWithUsage();
		fixture.taskbarWidget = {
			gap: 40,
			width: 320,
			show: {
				fiveHourBar: true,
				fiveHourPercent: true,
				fiveHourReset: true,
				weeklyBar: false,
				weeklyPercent: true,
				weeklyReset: true,
			},
			agents: [
				{ agent: 'claude', visible: true, profileId: null },
				{ agent: 'codex', visible: true, profileId: null },
			],
			layout: 'rows',
			labels: 'letter',
			fontPx: 11,
			theme: 'auto',
			backgroundOpacity: 0,
			colors: { normal: null, warn: '#E8A33D', critical: '#E5484D' },
			click: 'focus',
		};
		await installMockBridge(page, fixture);
		await page.goto('/?settings=usage');

		await page.getByRole('button', { name: 'Move Claude Code down' }).click();
		await page.getByTestId('settings-widget-reset').click();
		await page.getByTestId('settings-save').click();
		await expect(page.getByTestId('settings-modal')).toHaveCount(0);

		const calls = await page.evaluate(() => window.__FACTORAI_TEST_CALLS__ ?? []);
		const saved = calls.find((c) => c.name === 'set_taskbar_widget_config');
		const config = (saved?.args as { config: { agents: { agent: string }[] } }).config;
		expect(config).toMatchObject({ gap: 8, width: 244 });
		expect(config.agents.map((a) => a.agent)).toEqual(['codex', 'claude']);
	});

	test('@smoke a click on the widget opens the status bar details', async ({ page }) => {
		await installMockBridge(page, fixtureWithUsage());
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		await expect(page.getByTestId('status-bar')).toBeVisible();
		await expect(page.getByTestId('usage-popover')).toHaveCount(0);

		await page.evaluate(() => window.__FACTORAI_EMIT__?.('usage:open-details', null));
		await expect(page.getByTestId('usage-popover')).toBeVisible();
	});
});

test.describe('usage thresholds in the status bar', () => {
	test('@smoke the bars follow the saved thresholds', async ({ page }) => {
		const fixture = fixtureWithUsage();
		// Claude is at 62% in this fixture: neutral by default, red from 60.
		fixture.settings = { ...fixture.settings, usageWarnPercent: '40', usageCriticalPercent: '60' };
		await installMockBridge(page, fixture);
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);

		await expect(page.getByTestId('usage-fill-claude-5h')).toHaveClass(/bg-destructive/);
		// 31% weekly is under the new warning too.
		await expect(page.getByTestId('usage-fill-claude-wk')).toHaveClass(/bg-muted-foreground/);
	});
});
