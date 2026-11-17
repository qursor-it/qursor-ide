import { type Page, expect, test } from '@playwright/test';
import { FOO_ID, fixtureOneProjectOneSession, installMockBridge } from './fixtures';

/**
 * Canvas mode (specs/05-features.md F44): the third rail entry, a canvas list per
 * project, the create dialog, the board, its autosave and what happens when the
 * file changes under it.
 *
 * The browser bridge has no disk: `create_canvas` writes into the fixture's
 * `files`, and the Rust side (slug, `create_new`, folder) has its own tests.
 */
const ROOT = '/home/alice/code/foo';
const PATH = `${ROOT}/canvas/idea.canvas`;
const AGENT_NODE = {
	id: 'g1',
	type: 'text',
	x: 400,
	y: 0,
	width: 160,
	height: 160,
	text: 'from agent',
	'x-kind': 'sticky',
	'x-author': 'agent',
};

/** A fixture with one canvas file already on the "disk". */
function fixtureWithCanvas(doc: object) {
	const fx = fixtureOneProjectOneSession();
	fx.canvasesByProject = {
		[FOO_ID]: [
			{
				projectId: FOO_ID,
				path: PATH,
				relPath: 'canvas/idea.canvas',
				slug: 'idea',
				title: 'Idea',
				modifiedMs: 1,
			},
		],
	};
	fx.files = {
		...fx.files,
		[PATH]: { path: PATH, contents: JSON.stringify(doc, null, 2), isBinary: false } as never,
	};
	return fx;
}
const disk = (page: Page) =>
	page.evaluate((p) => window.__FACTORAI_TEST__?.files?.[p]?.contents ?? null, PATH);
/** Play the agent: change the file under the board, then let the watch fire. */
const agentWrites = (page: Page, doc: object | null, emit = true) =>
	page.evaluate(
		([p, text, fire]) => {
			const files = window.__FACTORAI_TEST__?.files ?? {};
			if (text === null) delete files[p];
			else files[p] = { path: p, contents: text, isBinary: false } as never;
			if (fire) window.__FACTORAI_EMIT__?.('canvas:changed', { path: p });
		},
		[PATH, doc === null ? null : JSON.stringify(doc, null, 2), emit] as const,
	);
async function openIdea(page: Page, doc: object = { title: 'Idea', nodes: [], edges: [] }) {
	await installMockBridge(page, fixtureWithCanvas(doc));
	await page.goto('/');
	await page.getByTestId('mode-canvas').click();
	await page.getByTestId('canvas-row').click();
	await expect(page.getByTestId('canvas-board')).toBeVisible();
}

test.describe('canvas mode', () => {
	test('@smoke creating a canvas lists it and opens its board', async ({ page }) => {
		await installMockBridge(page, fixtureOneProjectOneSession());
		await page.goto('/');
		await page.getByTestId('mode-canvas').click();
		await expect(page.getByTestId('canvas-mode-view')).toBeVisible();

		await page.getByTestId('new-canvas').click();
		await page.getByLabel('Canvas title').fill('Brainstorm MVP');
		await page.getByRole('button', { name: 'Create' }).click();

		await expect(page.getByTestId('canvas-row')).toHaveText('Brainstorm MVP');
		await expect(page.getByTestId('canvas-board')).toBeVisible();

		const saved = await page.evaluate(() =>
			Object.entries(window.__FACTORAI_TEST__?.files ?? {}).find(([p]) => p.endsWith('.canvas')),
		);
		expect(saved?.[0]).toBe('/home/alice/code/foo/canvas/brainstorm-mvp.canvas');
		expect(saved?.[1].contents).toContain('"title": "Brainstorm MVP"');
	});

	test('@smoke an unreadable canvas shows an error card, not a board', async ({ page }) => {
		const fx = fixtureOneProjectOneSession();
		const path = '/home/alice/code/foo/canvas/broken.canvas';
		fx.canvasesByProject = {
			[FOO_ID]: [
				{
					projectId: FOO_ID,
					path,
					relPath: 'canvas/broken.canvas',
					slug: 'broken',
					title: 'broken',
					modifiedMs: 1,
				},
			],
		};
		fx.files = { ...fx.files, [path]: { path, contents: '{', isBinary: false } as never };
		await installMockBridge(page, fx);
		await page.goto('/');
		await page.getByTestId('mode-canvas').click();
		await page.getByTestId('canvas-row').click();
		await expect(page.getByTestId('canvas-error')).toBeVisible();
		await expect(page.getByRole('button', { name: 'Open as text' })).toBeVisible();
		await expect(page.getByTestId('canvas-board')).toHaveCount(0);
	});
	test('@smoke a sticky added with N autosaves into the file', async ({ page }) => {
		await openIdea(page);
		await page.keyboard.press('n');
		await page.locator('.react-flow__pane').click({ position: { x: 400, y: 250 } });
		await page.keyboard.type('hello');
		await expect(page.getByTestId('canvas-save-state')).toHaveText('Saved');
		const saved = JSON.parse((await disk(page)) ?? '{}');
		expect(saved.nodes).toHaveLength(1);
		expect(saved.nodes[0]).toMatchObject({ text: 'hello', 'x-kind': 'sticky' });
		expect(saved.title).toBe('Idea');
	});

	test('@smoke a node the agent wrote appears with the AGENT badge', async ({ page }) => {
		await openIdea(page);
		await agentWrites(page, { title: 'Idea', nodes: [AGENT_NODE], edges: [] });
		await expect(page.getByText('from agent')).toBeVisible();
		await expect(page.getByText('AGENT', { exact: true })).toBeVisible();
		await expect(page.getByTestId('canvas-conflict')).toHaveCount(0);
	});

	test('@smoke unknown fields survive an edit', async ({ page }) => {
		const note = {
			id: 'a',
			type: 'text',
			x: 0,
			y: 0,
			width: 160,
			height: 160,
			text: 'x',
			'x-foo': 7,
		};
		await openIdea(page, { title: 'Idea', meta: { k: 1 }, nodes: [note], edges: [] });
		await page.getByText('x', { exact: true }).dblclick();
		await page.keyboard.type('yz');
		await expect(page.getByTestId('canvas-save-state')).toHaveText('Saved');
		const saved = JSON.parse((await disk(page)) ?? '{}');
		expect(saved.meta).toEqual({ k: 1 });
		expect(saved.nodes[0]).toMatchObject({ 'x-foo': 7, text: 'xyz' });
	});

	test('@smoke an edit meeting an agent write shows the conflict banner, never merges', async ({
		page,
	}) => {
		await openIdea(page);
		// The agent writes between the board's last read and its save: no event yet.
		await agentWrites(page, { title: 'Idea', nodes: [AGENT_NODE], edges: [] }, false);
		await page.keyboard.press('n');
		await page.locator('.react-flow__pane').click({ position: { x: 400, y: 250 } });
		await page.keyboard.type('mine');
		await expect(page.getByTestId('canvas-conflict')).toBeVisible();
		// The write was refused: the agent's version is still what is on disk.
		expect(await disk(page)).toContain('from agent');
		expect(await disk(page)).not.toContain('mine');

		await page.getByRole('button', { name: 'Keep mine' }).click();
		await expect(page.getByTestId('canvas-conflict')).toHaveCount(0);
		await expect(page.getByTestId('canvas-save-state')).toHaveText('Saved');
		expect(await disk(page)).toContain('mine');
	});

	test('@smoke Reload drops local edits for the disk version', async ({ page }) => {
		await openIdea(page);
		await agentWrites(page, { title: 'Idea', nodes: [AGENT_NODE], edges: [] }, false);
		await page.keyboard.press('n');
		await page.locator('.react-flow__pane').click({ position: { x: 400, y: 250 } });
		await page.keyboard.type('mine');
		await page.getByRole('button', { name: 'Reload' }).click();
		await expect(page.getByTestId('canvas-conflict')).toHaveCount(0);
		await expect(page.getByText('from agent')).toBeVisible();
		await expect(page.getByText('mine')).toHaveCount(0);
	});

	test('@smoke a canvas deleted on disk shows a banner and is not recreated', async ({ page }) => {
		await openIdea(page, {
			title: 'Idea',
			nodes: [{ ...AGENT_NODE, 'x-author': undefined }],
			edges: [],
		});
		await agentWrites(page, null);
		await expect(page.getByTestId('canvas-deleted')).toBeVisible();
		await page.getByText('from agent').dblclick();
		await page.keyboard.type('!');
		await page.waitForTimeout(1200);
		expect(await disk(page)).toBeNull();
	});
});
