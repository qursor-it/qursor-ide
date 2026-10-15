import { expect, type Locator, type Page, test } from '@playwright/test';
import { fixtureWithFileTree, installMockBridge } from './fixtures';

/**
 * Files and images reach the agent through the terminal (F5, ADR-0083).
 *
 * The drop is Tauri's native event, which the browser lane stands in for with
 * `__FACTORAI_EMIT__('drag-drop', …)`; the paste is a real `paste` event on the
 * xterm textarea. Both are observed where they leave the renderer, as the
 * `terminal_write` the mock records.
 */
const AGENT = 'agent-pty-input';
const SHELL = 'shell-pty-input';

async function openBoth(page: Page): Promise<{ agent: Locator; shell: Locator }> {
	await installMockBridge(page, {
		...fixtureWithFileTree(),
		terminalSpawnId: AGENT,
		shellSpawnId: SHELL,
	});
	await page.goto('/');
	await page.locator('aside').getByText('foo').click();
	await page.getByText('Refactor the auth middleware').click();
	await page.getByTestId('shell-footer').getByRole('button', { name: 'Terminal' }).click();
	const shell = page.getByTestId('shell-pane-host');
	await expect(shell).toBeVisible();
	const agent = page.locator('.xterm:visible').first().locator('xpath=..');
	await expect(agent).toBeVisible();
	return { agent, shell };
}

async function centre(target: Locator): Promise<{ x: number; y: number }> {
	const box = await target.boundingBox();
	if (!box) throw new Error('no box');
	// The browser lane runs at a device pixel ratio of 1, so physical is CSS.
	return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

async function emitDrag(
	page: Page,
	payload: { type: string; position?: { x: number; y: number }; paths?: string[] },
): Promise<void> {
	await page.evaluate((p) => window.__FACTORAI_EMIT__?.('drag-drop', p), payload);
}

function writes(page: Page, id: string): Promise<string[]> {
	return page.evaluate(
		(target) =>
			(window.__FACTORAI_TEST_CALLS__ ?? [])
				.filter((c) => c.name === 'terminal_write' && c.args?.id === target)
				.map((c) => String(c.args?.data)),
		id,
	);
}

function onWindows(page: Page): Promise<boolean> {
	return page.evaluate(() => navigator.userAgent.includes('Windows'));
}

async function pasteImage(page: Page, host: Locator): Promise<void> {
	await host.locator('textarea').evaluate((el) => {
		const data = new DataTransfer();
		data.items.add(
			new File([new Uint8Array([137, 80, 78, 71])], 'shot.png', { type: 'image/png' }),
		);
		el.dispatchEvent(
			new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
		);
	});
}

test.describe('terminal input', () => {
	test('@smoke a drop types the paths into the terminal under the pointer', async ({ page }) => {
		const { agent, shell } = await openBoth(page);
		const windows = await onWindows(page);
		const paths = windows ? ['C:\\tmp\\plain.png'] : ['/tmp/plain.png'];

		// Over the agent: it wears the outline, the shell does not.
		const over = await centre(agent);
		await emitDrag(page, { type: 'enter', paths, position: over });
		await expect(agent).toHaveAttribute('data-drop-target', '');
		await expect(shell).not.toHaveAttribute('data-drop-target', '');
		await emitDrag(page, { type: 'over', position: over });
		await emitDrag(page, { type: 'drop', paths, position: over });
		await expect(agent).not.toHaveAttribute('data-drop-target', '');
		await expect.poll(() => writes(page, AGENT)).toContain(`${paths[0]} `);
		expect(await writes(page, SHELL)).toEqual([]);

		// Then the shell.
		const inShell = await centre(shell.locator('.xterm'));
		await emitDrag(page, { type: 'drop', paths, position: inShell });
		await expect.poll(() => writes(page, SHELL)).toContain(`${paths[0]} `);
	});

	test('@smoke a path with a space is quoted for the platform', async ({ page }) => {
		const { agent } = await openBoth(page);
		const windows = await onWindows(page);
		const path = windows ? 'C:\\tmp\\my shot.png' : '/tmp/my shot.png';
		await emitDrag(page, { type: 'drop', paths: [path], position: await centre(agent) });
		const expected = windows ? `"${path}" ` : '/tmp/my\\ shot.png ';
		await expect.poll(() => writes(page, AGENT)).toContain(expected);
	});

	test('@smoke a drop away from a terminal, or a drag that leaves, does nothing', async ({
		page,
	}) => {
		const { agent } = await openBoth(page);
		await emitDrag(page, { type: 'over', position: await centre(agent) });
		await expect(agent).toHaveAttribute('data-drop-target', '');
		await emitDrag(page, { type: 'leave' });
		await expect(agent).not.toHaveAttribute('data-drop-target', '');

		// The sidebar is not a terminal.
		await emitDrag(page, {
			type: 'drop',
			paths: ['/tmp/x.png'],
			position: await centre(page.locator('aside')),
		});
		expect(await writes(page, AGENT)).toEqual([]);
		expect(await writes(page, SHELL)).toEqual([]);
	});

	test("@smoke pasting an image in Claude's terminal sends Claude's image-paste key", async ({
		page,
	}) => {
		const { agent, shell } = await openBoth(page);
		const key = (await onWindows(page)) ? '\x1bv' : '\x16';

		await pasteImage(page, agent);
		await expect.poll(() => writes(page, AGENT)).toEqual([key]);

		// A shell has no such key: xterm's own, empty, text paste is all it gets.
		await pasteImage(page, shell);
		await expect.poll(() => writes(page, SHELL)).toEqual(['']);
	});

	test('@smoke pasting text still pastes it', async ({ page }) => {
		const { agent } = await openBoth(page);
		await agent.locator('textarea').evaluate((el) => {
			const data = new DataTransfer();
			data.setData('text/plain', 'hello there');
			el.dispatchEvent(
				new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
			);
		});
		await expect.poll(() => writes(page, AGENT)).toEqual(['hello there']);
	});

	test('@smoke Ctrl+V in Claude pastes text on Windows and is the image key elsewhere', async ({
		context,
		page,
	}) => {
		await context.grantPermissions(['clipboard-read', 'clipboard-write']);
		const { agent } = await openBoth(page);
		await page.evaluate(() => navigator.clipboard.writeText('from the clipboard'));
		await agent.locator('textarea').focus();
		await page.keyboard.press('Control+V');
		const expected = (await onWindows(page)) ? 'from the clipboard' : '\x16';
		await expect.poll(() => writes(page, AGENT)).toEqual([expected]);
	});
});
