import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CanvasDocController, acquireCanvasDoc, heldCanvases } from './useCanvasDoc';

const PATH = '/p/canvas/x.canvas';
const node = (id: string, x = 0) => ({
	id,
	type: 'text',
	x,
	y: 0,
	width: 160,
	height: 160,
	text: id,
	'x-kind': 'sticky',
});
const file = (...ids: string[]) =>
	JSON.stringify({ title: 't', nodes: ids.map((id, i) => node(id, i * 200)), edges: [] });

function setup(initial = file('a')) {
	let disk = { contents: initial, sha256: 'sha-1' };
	let emit: (e: { path: string }) => void = () => {};
	const api = {
		readCanvas: vi.fn(async () => disk),
		writeCanvas: vi.fn(async (_p: string, _path: string, _c: string, _sha: string | null) => ({
			kind: 'Written' as const,
			sha256: 'sha-2',
		})),
		watchCanvas: vi.fn(async () => {}),
		unwatchCanvas: vi.fn(async () => true),
		onCanvasChanged: vi.fn(async (cb: (e: { path: string }) => void) => {
			emit = cb;
			return () => {};
		}),
	};
	const ctl = new CanvasDocController('p', PATH, api as never);
	return {
		api,
		ctl,
		setDisk: (contents: string, sha256: string) => {
			disk = { contents, sha256 };
		},
		fire: () => emit({ path: PATH }),
	};
}
const tick = (ms = 0) => vi.advanceTimersByTimeAsync(ms);
const move = (ctl: CanvasDocController) =>
	ctl.update((s) => ({
		...s,
		nodes: s.nodes.map((n) => ({ ...n, position: { x: 9, y: 9 } })),
	}));

describe('CanvasDocController', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => {
		for (const c of heldCanvases.values()) c.release();
		vi.useRealTimers();
	});

	it('autosaves_after_600ms', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		move(t.ctl);
		await tick(599);
		expect(t.api.writeCanvas).not.toHaveBeenCalled();
		await tick(1);
		expect(t.api.writeCanvas).toHaveBeenCalledTimes(1);
		expect(t.api.writeCanvas.mock.calls[0]?.[3]).toBe('sha-1');
		expect(t.ctl.getSnapshot().saveState).toBe('saved');
	});

	it('conflict_on_refused_write', async () => {
		const t = setup();
		t.api.writeCanvas.mockResolvedValueOnce({ kind: 'Conflict', diskSha256: 'other' } as never);
		t.ctl.start();
		await tick();
		move(t.ctl);
		await tick(600);
		const s = t.ctl.getSnapshot();
		expect(s.saveState).toBe('conflict');
		expect(s.nodes[0]?.position).toEqual({ x: 9, y: 9 });
		// Nothing is retried behind the banner.
		await tick(5000);
		expect(t.api.writeCanvas).toHaveBeenCalledTimes(1);
	});

	it('external_change_clean_reload_marks_new', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		t.setDisk(file('a', 'g1'), 'sha-9');
		t.fire();
		await tick();
		const s = t.ctl.getSnapshot();
		expect(s.nodes.map((n) => n.id)).toContain('g1');
		expect(s.newIds.has('g1')).toBe(true);
		expect(s.saveState).toBe('saved');
	});

	it('external_change_dirty_sets_conflict', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		move(t.ctl);
		t.setDisk(file('a', 'g1'), 'sha-9');
		t.fire();
		await tick();
		const s = t.ctl.getSnapshot();
		expect(s.saveState).toBe('conflict');
		expect(s.conflict).not.toBeNull();
		expect(s.nodes.map((n) => n.id)).toEqual(['a']);
		// Keep mine writes against the disk's hash.
		s.conflict?.keepMine();
		await tick();
		expect(t.api.writeCanvas.mock.calls[0]?.[3]).toBe('sha-9');
	});

	it('own_write_echo_ignored', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		move(t.ctl);
		await tick(600);
		t.setDisk('{"nodes":[],"edges":[]}', 'sha-2');
		t.fire();
		await tick();
		const s = t.ctl.getSnapshot();
		expect(s.saveState).toBe('saved');
		expect(s.nodes).toHaveLength(1);
		expect(s.conflict).toBeNull();
	});

	it('no_write_when_invalid_or_deleted', async () => {
		const bad = setup('{');
		bad.ctl.start();
		await tick();
		expect(bad.ctl.getSnapshot().status).toBe('invalid');
		move(bad.ctl);
		await tick(2000);
		expect(bad.api.writeCanvas).not.toHaveBeenCalled();

		const gone = setup();
		gone.ctl.start();
		await tick();
		gone.api.readCanvas.mockRejectedValue({ kind: 'NotFound', message: 'gone' });
		gone.fire();
		await tick();
		expect(gone.ctl.getSnapshot().status).toBe('deleted');
		move(gone.ctl);
		await tick(2000);
		expect(gone.api.writeCanvas).not.toHaveBeenCalled();
	});

	it('flushes_pending_edit_on_dispose_and_unwatches', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		move(t.ctl);
		t.ctl.dispose();
		await tick();
		expect(t.api.writeCanvas).toHaveBeenCalledTimes(1);
		expect(t.api.unwatchCanvas).toHaveBeenCalledWith(PATH);
	});

	it('undo_and_redo_step_through_edits', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		move(t.ctl);
		t.ctl.undo();
		expect(t.ctl.getSnapshot().nodes[0]?.position).toEqual({ x: 0, y: 0 });
		t.ctl.redo();
		expect(t.ctl.getSnapshot().nodes[0]?.position).toEqual({ x: 9, y: 9 });
	});

	it('dispose_in_conflict_keeps_local_state', async () => {
		const t = setup();
		t.api.writeCanvas.mockResolvedValueOnce({ kind: 'Conflict', diskSha256: 'other' } as never);
		t.ctl.start();
		await tick();
		move(t.ctl);
		await tick(600);
		t.ctl.dispose();
		await tick();
		expect(t.api.unwatchCanvas).not.toHaveBeenCalled();
		const again = acquireCanvasDoc('p', PATH, t.api as never);
		expect(again).toBe(t.ctl);
		again.start();
		await tick();
		const s = again.getSnapshot();
		expect(s.saveState).toBe('conflict');
		expect(s.nodes[0]?.position).toEqual({ x: 9, y: 9 });
	});

	it('dispose_clean_releases', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		t.ctl.dispose();
		await tick();
		expect(heldCanvases.size).toBe(0);
		expect(t.api.unwatchCanvas).toHaveBeenCalledWith(PATH);
	});

	it('failed_final_save_is_kept', async () => {
		const t = setup();
		t.ctl.start();
		await tick();
		move(t.ctl);
		t.api.writeCanvas.mockRejectedValueOnce(new Error('disk full'));
		t.ctl.dispose();
		await tick();
		expect(heldCanvases.get(PATH)).toBe(t.ctl);
		const again = acquireCanvasDoc('p', PATH, t.api as never);
		again.start();
		await tick();
		const s = again.getSnapshot();
		expect(s.saveState).not.toBe('saved');
		expect(s.nodes[0]?.position).toEqual({ x: 9, y: 9 });
	});
});
