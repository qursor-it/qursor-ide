/**
 * F41 — a commit in Git mode names the session that was running in the
 * checkout when it was made.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureWithGraph, installMockBridge } from './fixtures';

const COMMIT_TIME = 1_760_000_000_000;

function fixture(window: { from: number; to: number }) {
	const fx = fixtureWithGraph();
	const root = fx.projects?.[0]?.realPath ?? '';
	const sessions = fx.sessionsByProject?.[FOO_ID] ?? [];
	fx.sessionsByProject = {
		[FOO_ID]: sessions.map((s, i) =>
			i === 0
				? { ...s, cwd: root, worktree: null, createdAt: window.from, updatedAt: window.to }
				: s,
		),
	};
	return fx;
}

test.describe('commit session', () => {
	test('@smoke names the session running when the commit was made and opens it', async ({
		page,
	}) => {
		await installMockBridge(
			page,
			fixture({ from: COMMIT_TIME - 60_000, to: COMMIT_TIME + 60_000 }),
		);
		await page.goto(`/#/projects/${FOO_ID}`);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('commit-row').nth(1).click();

		const strip = page.getByTestId('commit-session');
		await expect(strip).toContainText('Refactor the auth middleware');
		await strip.getByRole('button', { name: 'Open session' }).click();
		await expect(page).toHaveURL(/sessions\/session-uuid-001/);
	});

	test('@smoke says nothing when no session was running then', async ({ page }) => {
		await installMockBridge(
			page,
			fixture({ from: COMMIT_TIME + 3_600_000, to: COMMIT_TIME + 7_200_000 }),
		);
		await page.goto(`/#/projects/${FOO_ID}`);
		await page.getByRole('tab', { name: /^Git/ }).click();
		await page.getByTestId('commit-row').nth(1).click();
		await expect(page.getByTestId('commit-detail')).toBeVisible();
		await expect(page.getByTestId('commit-session')).toHaveCount(0);
	});
});
