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

test.describe('session plan and cost', () => {
	test('@smoke shows the agent plan and the cost in the Review tab', async ({ page }) => {
		const fx = fixtureWithChanges();
		fx.sessionPlans = {
			'session-uuid-001': [
				{ content: 'Find where the duplicate is created', status: 'completed' },
				{ content: 'Add a retry test', status: 'in_progress' },
				{ content: 'Run the webhook suite', status: 'pending' },
			],
		};
		fx.sessionUsage = {
			'session-uuid-001': {
				sessionId: 'session-uuid-001',
				model: 'claude-opus',
				inputTokens: 1000,
				outputTokens: 500,
				cacheReadTokens: 0,
				cacheWriteTokens: 0,
				reasoningTokens: 0,
				totalTokens: 1500,
				costUsd: 0.84,
			},
		};
		await installMockBridge(page, fx);
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		// The plan and the cost live in the panel's Review tab (ADR-0094).
		await page.getByRole('button', { name: 'Toggle file tree' }).click();
		await page.getByRole('tab', { name: 'Review' }).click();

		const review = page.getByTestId('review-view');
		await expect(review).toContainText('1/3');
		const list = page.getByTestId('session-plan-list');
		await expect(list).toContainText('Add a retry test');
		await expect(list.getByLabel('Done')).toHaveCount(1);
		await expect(page.getByTestId('session-cost')).toContainText('0.84');
	});

	test('@smoke a session with no plan shows no plan', async ({ page }) => {
		await installMockBridge(page, fixtureWithChanges());
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await page.getByRole('button', { name: 'Toggle file tree' }).click();
		await page.getByRole('tab', { name: 'Review' }).click();
		await expect(page.getByTestId('review-view')).toContainText('Changed files');
		await expect(page.getByTestId('session-plan-list')).toHaveCount(0);
	});
});

test.describe('changes chip refresh', () => {
	test('@smoke reads git again each time a session is shown, with no timer', async ({ page }) => {
		const fx = fixtureWithChanges();
		const [first] = fx.sessionsByProject?.[FOO_ID] ?? [];
		fx.sessionsByProject = {
			[FOO_ID]: [first, { ...first, id: 'session-uuid-002', title: 'Second session' }],
		};
		await installMockBridge(page, fx);
		const statusCalls = () =>
			page.evaluate(
				() => (window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === 'git_status').length,
			);

		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		await expect(page.getByTestId('session-changes')).toBeVisible();
		const afterFirst = await statusCalls();

		// Nothing polls while you stay on it.
		await page.waitForTimeout(1500);
		expect(await statusCalls()).toBe(afterFirst);

		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-002`);
		await expect(page.getByTestId('session-changes')).toBeVisible();
		await expect.poll(statusCalls).toBeGreaterThan(afterFirst);
		const afterSecond = await statusCalls();

		// Back to the first, same checkout: read again.
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		await expect.poll(statusCalls).toBeGreaterThan(afterSecond);
	});
});

test.describe('review tab', () => {
	test('@smoke lists the changed files and opens one as a diff; the button goes to Changes', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureWithChanges());
		await page.goto(`/#/projects/${FOO_ID}/sessions/session-uuid-001`);
		await page.getByRole('button', { name: 'Toggle file tree' }).click();
		await page.getByRole('tab', { name: 'Review' }).click();

		const files = page.getByTestId('review-files');
		await expect(files.getByText('new-file.ts')).toBeVisible();
		await files.getByText('new-file.ts').click();
		await expect(page).toHaveURL(/file=.*new-file\.ts/);

		await page.getByTestId('review-all-changes').click();
		await expect(page.getByRole('tab', { name: 'Changes' })).toHaveAttribute(
			'aria-selected',
			'true',
		);
	});
});
