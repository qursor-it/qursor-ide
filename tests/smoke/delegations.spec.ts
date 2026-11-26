import { expect, test } from '@playwright/test';
import { ALPHA_ID, fixtureTwoProjectsManySessions, installMockBridge } from './fixtures';

/**
 * Cross-agent delegates in the UI (specs/05-features.md F50): a live one is
 * shown under the session that delegated, and a finished one is disposable.
 */
const PARENT = 'alpha-session-1';
const CHILD = 'aaaaaaaa-0000-4000-8000-0000000000d1';

function fixture() {
	return {
		...fixtureTwoProjectsManySessions(),
		terminalSpawnId: 'pty-parent',
		opencodeCli: { installed: true, binaryPath: '/usr/local/bin/opencode', version: '1.18.30' },
		delegations: {
			active: [
				{
					id: CHILD,
					parentSessionId: PARENT,
					sessionId: CHILD,
					projectId: ALPHA_ID,
					agent: 'opencode' as const,
					prompt: 'Write the regression test',
					status: 'running' as const,
					createdAt: Date.now(),
				},
			],
			retired: [],
		},
	};
}

test('@smoke a live delegate hangs under its parent and leaves when it finishes', async ({
	page,
}) => {
	await installMockBridge(page, fixture());
	await page.goto(`/#/projects/${ALPHA_ID}/sessions/${PARENT}`);

	const strip = page.getByTestId('delegates-strip');
	await expect(strip).toContainText('Delegated to 1 agent');
	await expect(page.getByTestId(`delegate-${CHILD}`)).toContainText('Write the regression test');

	await page.getByRole('button', { name: 'Expand alpha' }).click();
	const nested = page.getByTestId(`sidebar-delegate-${CHILD}`);
	await expect(nested.getByRole('img', { name: 'OpenCode' })).toBeVisible();

	// Into the child: its origin strip leads back.
	await page.evaluate(() => {
		if (window.__FACTORAI_TEST__) window.__FACTORAI_TEST__.terminalSpawnId = 'pty-child';
	});
	await nested.click();
	await expect(page.getByTestId('delegation-origin-strip')).toContainText('delegated by');
	const tabs = page.getByTestId('session-tabs').getByRole('tab');
	await expect(tabs.filter({ hasText: 'Write the regression test' })).toHaveCount(1);

	// It finishes: Rust retires it, and every trace of it goes.
	await page.evaluate(
		({ child, parent, projectId }) => {
			const fx = window.__FACTORAI_TEST__;
			if (fx)
				fx.delegations = { active: [], retired: [{ sessionId: child, parentSessionId: parent }] };
			window.__FACTORAI_EMIT__?.('delegations:changed', { projectId });
		},
		{ child: CHILD, parent: PARENT, projectId: ALPHA_ID },
	);
	await expect(page).toHaveURL(new RegExp(`sessions/${PARENT}$`));
	await expect(tabs.filter({ hasText: 'Write the regression test' })).toHaveCount(0);
	await expect(page.getByTestId('delegates-strip')).toHaveCount(0);
	await expect(page.getByTestId(`sidebar-delegate-${CHILD}`)).toHaveCount(0);
});

test('@smoke the approval names the session that asked', async ({ page }) => {
	await installMockBridge(page, fixture());
	await page.goto(`/#/projects/${ALPHA_ID}`);
	await page.evaluate(
		({ projectId, parent }) =>
			window.__FACTORAI_EMIT__?.('agent-session:start', {
				delegationId: 'bbbbbbbb-0000-4000-8000-0000000000d2',
				parentSessionId: parent,
				projectId,
				agent: 'opencode',
				cwd: '/workspace/alpha',
				prompt: 'Draft the changelog',
			}),
		{ projectId: ALPHA_ID, parent: PARENT },
	);
	const dialog = page.getByRole('dialog', { name: 'Delegate to OpenCode' });
	await expect(dialog).toContainText('Requested by Alpha only task');
});

test('@smoke closing a parent asks first and names the delegates that close with it', async ({
	page,
}) => {
	await installMockBridge(page, fixture());
	await page.goto(`/#/projects/${ALPHA_ID}/sessions/${PARENT}`);
	await expect(page.locator('.xterm:visible')).toBeVisible();
	// Not working, so a plain session would close on the click.
	await page.evaluate(() =>
		window.__FACTORAI_EMIT__?.('terminal:status', {
			id: 'pty-parent',
			status: 'waiting_input',
			lastActivity: Date.now(),
		}),
	);
	await page.getByRole('button', { name: 'Close session' }).click();
	const dialog = page.getByRole('dialog', { name: 'Close this session?' });
	await expect(dialog).toContainText('Its delegated session closes with it, first.');
	await dialog.getByRole('button', { name: 'Keep it running' }).click();
	await expect(dialog).toHaveCount(0);
});
