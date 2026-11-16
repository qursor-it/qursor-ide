/**
 * F43 — reviewing a session's changes on one screen.
 */
import { expect, test } from '@playwright/test';
import { FOO_ID, fixtureWithChanges, installMockBridge } from './fixtures';

const SESSION = 'session-uuid-001';
const REVIEW = `/#/projects/${FOO_ID}/sessions/${SESSION}/review`;

function fixture() {
	const fx = fixtureWithChanges();
	fx.sessionPages = {
		...fx.sessionPages,
		[SESSION]: {
			id: SESSION,
			offset: 0,
			limit: 40,
			total: 3,
			events: [
				{
					type: 'assistant',
					message: {
						role: 'assistant',
						content: [
							{
								type: 'tool_use',
								id: 't1',
								name: 'Bash',
								input: { command: 'pnpm test webhooks' },
							},
						],
					},
				},
				{
					type: 'user',
					message: {
						role: 'user',
						content: [{ type: 'tool_result', tool_use_id: 't1', content: 'ok' }],
					},
				},
				{
					type: 'assistant',
					message: {
						role: 'assistant',
						content: [{ type: 'text', text: 'Both writes now share one transaction.' }],
					},
				},
			],
		},
	};
	fx.sessionPlans = { [SESSION]: [{ content: 'Add a retry test', status: 'in_progress' }] };
	return fx;
}

test.describe('review mode', () => {
	test('@smoke lists the files, shows a diff, what the agent said and its plan', async ({
		page,
	}) => {
		await installMockBridge(page, fixture());
		await page.goto(REVIEW);
		const review = page.getByTestId('review-mode');
		await expect(review).toContainText('Review changes');
		await review.getByTestId('review-file').filter({ hasText: 'src/index.ts' }).first().click();
		await expect(page.getByTestId('diff-view-editor')).toBeVisible();
		await expect(page.getByTestId('review-agent-said')).toHaveText(
			'Both writes now share one transaction.',
		);
		await expect(page.getByTestId('session-plan-list')).toContainText('Add a retry test');
		await expect(page.getByTestId('review-checks')).toContainText('pnpm test webhooks');
		await expect(page.getByTestId('review-checks').getByLabel('Passed')).toBeVisible();
	});

	test('@smoke ticks files off as reviewed', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto(REVIEW);
		const progress = page.getByTestId('review-progress');
		await expect(progress).toHaveText(/^0 \//);
		await page.getByRole('checkbox').first().click();
		await expect(progress).toHaveText(/^1 \//);
	});

	test('@smoke Back to session and Esc return to the session; Commit opens Git', async ({
		page,
	}) => {
		await installMockBridge(page, fixture());
		await page.goto(REVIEW);
		await page.getByRole('button', { name: 'Back to session' }).click();
		await expect(page).toHaveURL(new RegExp(`sessions/${SESSION}$`));

		await page.goto(REVIEW);
		await expect(page.getByTestId('review-mode')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(page).toHaveURL(new RegExp(`sessions/${SESSION}$`));

		await page.goto(REVIEW);
		await page.getByTestId('review-commit').click();
		await expect(page.getByTestId('git-mode-view')).toBeVisible();
	});

	test('@smoke a note goes to the agent as a reply about the file', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto(`/#/projects/${FOO_ID}/sessions/${SESSION}`);
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await page.goto(REVIEW);
		await page.getByTestId('review-note').fill('cover charge.refunded too');
		await page.getByTestId('review-send').click();
		await expect(page).toHaveURL(new RegExp(`sessions/${SESSION}$`));
		const written = await page.evaluate(() =>
			(window.__FACTORAI_TEST_CALLS__ ?? [])
				.filter((c) => c.name === 'terminal_write')
				.map((c) => (c.args as { data: string }).data)
				.join(''),
		);
		expect(written).toContain('cover charge.refunded too');
		expect(written).toMatch(/About .+: cover charge\.refunded too/);
	});

	test('@smoke a note is anchored to the lines picked in the diff', async ({ page }) => {
		await installMockBridge(page, fixture());
		await page.goto(`/#/projects/${FOO_ID}/sessions/${SESSION}`);
		await expect(page.locator('.xterm:visible')).toBeVisible();
		await page.goto(REVIEW);
		const review = page.getByTestId('review-mode');
		await review.getByTestId('review-file').filter({ hasText: 'src/index.ts' }).first().click();
		const note = page.getByTestId('review-note');
		await expect(note).toHaveAttribute('placeholder', /^About [^,]+…$/);
		await page.getByTestId('diff-view-editor').locator('.modified .view-line').first().click();
		await expect(note).toHaveAttribute('placeholder', /, line 1…$/);
		await note.fill('rename this');
		await page.getByTestId('review-send').click();
		const written = await page.evaluate(() =>
			(window.__FACTORAI_TEST_CALLS__ ?? [])
				.filter((c) => c.name === 'terminal_write')
				.map((c) => (c.args as { data: string }).data)
				.join(''),
		);
		expect(written).toMatch(/About .+, line 1: rename this/);
	});
});
