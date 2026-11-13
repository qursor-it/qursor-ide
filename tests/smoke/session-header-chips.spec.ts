/**
 * F3 — the session header's second row: the state in words, and what the agent
 * has changed, as a door onto the Changes panel.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureWithChanges, installMockBridge } from './fixtures';

test.describe('session header', () => {
	test('@smoke says the state in words and opens Changes from its diff chip', async ({ page }) => {
		await installMockBridge(page, fixtureWithChanges());
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		await expect(page.locator('.xterm:visible')).toBeVisible();

		await expect(page.getByTestId('session-status-pill')).toHaveText('Working');
		const chip = page.getByTestId('session-changes');
		// Six rows in the fixture; their counts add up to +19 −2.
		await expect(chip).toContainText('6 files');
		await expect(chip).toContainText('+19');
		await expect(chip).toContainText('−2');

		await expect(page.getByTestId('file-tree-panel')).toHaveCount(0);
		await chip.click();
		await expect(page.getByTestId('file-tree-panel')).toBeVisible();
		await expect(page.getByRole('tab', { name: 'Changes' })).toHaveAttribute(
			'aria-selected',
			'true',
		);
	});
});
