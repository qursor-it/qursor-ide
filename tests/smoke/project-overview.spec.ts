/**
 * F2 — the top of a project page: what is live, what wants you, what is
 * uncommitted, and the rules the agent runs under.
 */
import { expect, test } from '@playwright/test';
import {
	FOO_ID,
	fixtureOneProjectOneSession,
	fixtureWithChanges,
	installMockBridge,
} from './fixtures';

test.describe('project overview', () => {
	test('@smoke counts uncommitted files and opens CLAUDE.md when there is one', async ({
		page,
	}) => {
		const fx = fixtureWithChanges();
		const root = fx.projects?.[0]?.realPath ?? '';
		const any = Object.values(fx.files ?? {})[0];
		const rules = '# Rules\n';
		fx.files = {
			...fx.files,
			[`${root}/CLAUDE.md`]: {
				...any,
				path: `${root}/CLAUDE.md`,
				contents: rules,
				size: rules.length,
			},
		};
		await installMockBridge(page, fx);
		await page.goto(`/#/projects/${FOO_ID}`);

		await expect(page.getByTestId('project-stat-uncommitted')).toContainText('6');
		await expect(page.getByTestId('project-stat-working')).toContainText('0');
		const card = page.getByTestId('project-rules');
		await card.getByRole('button', { name: 'Open CLAUDE.md' }).click();
		await expect(page).toHaveURL(/file=.*CLAUDE\.md/);
	});

	test('@smoke says so when the folder has no CLAUDE.md and is not a repository', async ({
		page,
	}) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto(`/#/projects/${FOO_ID}`);
		await expect(page.getByTestId('project-rules')).toContainText('No CLAUDE.md');
		await expect(page.getByTestId('project-rules').getByRole('button')).toHaveCount(0);
		await expect(page.getByTestId('project-stat-uncommitted')).toContainText('Not a repository');
	});
});
