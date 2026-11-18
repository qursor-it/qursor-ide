import { formatError } from '@lib/errors';
import { cmd, events } from '@lib/tauri';
import type { Edge, EdgeChangeset, Node, NodeChangeset } from '@xyflow/react';
import { useEffect, useMemo, useSyncExternalStore } from 'react';
import { createHistory } from './history';
import {
	type CanvasDoc,
	type CanvasEdgeData,
	type CanvasNodeData,
	newNodeIds,
	parseCanvas,
	serializeCanvas,
} from './jsonCanvas';

type CN = Node<CanvasNodeData>;
type CE = Edge<CanvasEdgeData>;
type Graph = { nodes: CN[]; edges: CE[] };
type CanvasStatus = 'loading' | 'ready' | 'invalid' | 'deleted';
export type SaveState = 'saved' | 'saving' | 'unsaved' | 'conflict';

interface CanvasSnapshot extends Graph {
	status: CanvasStatus;
	error: string | null;
	saveState: SaveState;
	newIds: Set<string>;
	/** Nodes the last external reload added (not the initial load, not the user's own); `seq` bumps per reload. */
	arrived: { seq: number; ids: Set<string> };
	conflict: null | { reload(): void; keepMine(): void };
}

type CanvasApi = Pick<typeof cmd, 'readCanvas' | 'writeCanvas' | 'watchCanvas' | 'unwatchCanvas'> &
	Pick<typeof events, 'onCanvasChanged'>;

const AUTOSAVE_MS = 600;
/**
 * Controllers that could not leave clean (a conflict, a failed final save) outlive
 * their view, keyed by canvas path, so reopening shows the local version again.
 */
const held = new Map<string, CanvasDocController>();
export const heldCanvases: ReadonlyMap<string, CanvasDocController> = held;
const heldListeners = new Set<() => void>();
let heldPaths: string[] = [];
const publishHeld = () => {
	heldPaths = [...held.keys()];
	for (const fn of heldListeners) fn();
};

/** Paths of canvases with unsaved or conflicting edits kept after navigating away. */
export function useHeldCanvasPaths(): string[] {
	return useSyncExternalStore(
		(fn) => {
			heldListeners.add(fn);
			return () => {
				heldListeners.delete(fn);
			};
		},
		() => heldPaths,
	);
}

/** The kept controller for this path, else a new one. */
export function acquireCanvasDoc(projectId: string, path: string, api?: CanvasApi) {
	return held.get(path) ?? new CanvasDocController(projectId, path, api);
}

const isNotFound = (e: unknown) => (e as { kind?: string } | null)?.kind === 'NotFound';

/**
 * The open canvas: its state, its autosave and its watch (F44). Framework-free so
 * the save rules are testable; `useCanvasDoc` is the React door.
 *
 * Rules that must hold: never write while `invalid` or `deleted` (nothing is
 * ever recreated), never merge (unsaved edits plus a disk change is a conflict
 * the user resolves), and a re-read whose sha is the one we just wrote is our
 * own echo.
 */
export class CanvasDocController {
	private snap: CanvasSnapshot = {
		nodes: [],
		edges: [],
		status: 'loading',
		error: null,
		saveState: 'saved',
		newIds: new Set(),
		arrived: { seq: 0, ids: new Set() },
		conflict: null,
	};
	private listeners = new Set<() => void>();
	private doc: CanvasDoc | null = null;
	private baseSha: string | null = null;
	private diskSha: string | null = null;
	private dirty = false;
	private timer: ReturnType<typeof setTimeout> | undefined;
	// Writes and re-reads queue behind this, so a re-read never races our own write.
	private inflight: Promise<void> = Promise.resolve();
	private history = createHistory<Graph>({ nodes: [], edges: [] });
	private unlisten: (() => void) | undefined;
	private stopped = true;

	constructor(
		private projectId: string,
		private path: string,
		private api: CanvasApi = { ...cmd, onCanvasChanged: events.onCanvasChanged },
	) {}

	subscribe = (fn: () => void) => {
		this.listeners.add(fn);
		return () => {
			this.listeners.delete(fn);
		};
	};
	getSnapshot = () => this.snap;
	private set(patch: Partial<CanvasSnapshot>) {
		this.snap = { ...this.snap, ...patch };
		for (const fn of this.listeners) fn();
	}

	start() {
		// Resuming a kept controller: its listener is still registered, but the
		// backend watch may have moved on, so watch again and catch up with the disk.
		if (held.get(this.path) === this) {
			held.delete(this.path);
			publishHeld();
		}
		if (!this.stopped) {
			this.api.watchCanvas(this.path).catch(() => {});
			void this.queue(() => this.reread());
			return;
		}
		this.stopped = false;
		this.api.watchCanvas(this.path).catch(() => {});
		this.api
			.onCanvasChanged((e) => {
				if (e.path === this.path) void this.queue(() => this.reread());
			})
			.then((un) => {
				if (this.stopped) un();
				else this.unlisten = un;
			});
		void this.queue(() => this.load());
	}

	/** Saves what is pending; lets go only if that left nothing unsaved or in conflict. */
	dispose() {
		held.set(this.path, this);
		publishHeld();
		void this.saveNow().then(() => {
			const clean = !this.dirty && this.snap.saveState === 'saved';
			// Reopened meanwhile (start() took it back out): not ours to release.
			if (clean && held.get(this.path) === this) this.release();
		});
	}

	/** Stops watching and forgets the controller. */
	release() {
		this.stopped = true;
		clearTimeout(this.timer);
		this.unlisten?.();
		this.unlisten = undefined;
		this.api.unwatchCanvas(this.path).catch(() => {});
		if (held.get(this.path) === this) {
			held.delete(this.path);
			publishHeld();
		}
	}

	private queue(job: () => Promise<void>): Promise<void> {
		this.inflight = this.inflight.then(job);
		return this.inflight;
	}

	private async load() {
		try {
			const { contents, sha256 } = await this.api.readCanvas(this.path);
			this.adopt(sha256, parseCanvas(contents));
		} catch (e) {
			if (isNotFound(e)) this.set({ status: 'deleted', error: null });
			else this.set({ status: 'invalid', error: formatError(e) });
		}
	}

	/** Replace the board with what is on disk. */
	private adopt(sha: string, parsed: ReturnType<typeof parseCanvas>, external = false) {
		clearTimeout(this.timer);
		this.dirty = false;
		this.baseSha = sha;
		if (!parsed.ok) {
			this.doc = null;
			this.set({ status: 'invalid', error: parsed.error, nodes: [], edges: [], conflict: null });
			return;
		}
		const added = newNodeIds(this.doc, parsed.doc);
		this.doc = parsed.doc;
		this.history.reset({ nodes: parsed.nodes, edges: parsed.edges });
		this.set({
			status: 'ready',
			error: null,
			nodes: parsed.nodes,
			edges: parsed.edges,
			saveState: 'saved',
			conflict: null,
			newIds: new Set([...this.snap.newIds, ...added]),
			...(external && added.size
				? { arrived: { seq: this.snap.arrived.seq + 1, ids: added } }
				: {}),
		});
	}

	private async reread() {
		let read: { contents: string; sha256: string };
		try {
			read = await this.api.readCanvas(this.path);
		} catch (e) {
			if (isNotFound(e)) {
				clearTimeout(this.timer);
				this.set({ status: 'deleted', error: null });
			}
			return;
		}
		// Our own write coming back, or nothing that matters.
		if (read.sha256 === this.baseSha && this.snap.status !== 'deleted') return;
		if (this.dirty || this.snap.saveState === 'conflict') {
			this.diskSha = read.sha256;
			this.setConflict();
			return;
		}
		this.adopt(read.sha256, parseCanvas(read.contents), true);
	}

	private setConflict() {
		clearTimeout(this.timer);
		this.set({
			status: this.snap.status === 'deleted' ? 'ready' : this.snap.status,
			saveState: 'conflict',
			conflict: { reload: this.reload, keepMine: this.keepMine },
		});
	}

	/** Drop local edits and take the disk's version. */
	reload = () => {
		void this.queue(() => this.load());
	};

	/** Overwrite the disk's version, if it is still the one the user was told about. */
	keepMine = () => {
		if (this.diskSha === null) return;
		this.baseSha = this.diskSha;
		this.dirty = true;
		this.set({ saveState: 'unsaved', conflict: null });
		void this.saveNow();
	};

	private markDirty() {
		this.dirty = true;
		if (this.snap.saveState === 'conflict') return;
		this.set({ saveState: 'unsaved' });
		clearTimeout(this.timer);
		this.timer = setTimeout(() => void this.saveNow(), AUTOSAVE_MS);
	}

	saveNow = (): Promise<void> => {
		clearTimeout(this.timer);
		return this.queue(() => this.write());
	};

	private async write() {
		const { status, saveState, nodes, edges } = this.snap;
		if (status !== 'ready' || !this.dirty || saveState === 'conflict' || !this.doc) return;
		const contents = serializeCanvas(this.doc, nodes, edges);
		this.dirty = false;
		this.set({ saveState: 'saving' });
		try {
			const out = await this.api.writeCanvas(this.projectId, this.path, contents, this.baseSha);
			if (out.kind === 'Written') {
				this.baseSha = out.sha256;
				// Edits made while the write was in flight are still pending.
				if (this.dirty) this.markDirty();
				else this.set({ saveState: 'saved' });
			} else {
				this.dirty = true;
				this.diskSha = out.diskSha256;
				if (out.diskSha256 === null) this.set({ status: 'deleted' });
				else this.setConflict();
			}
		} catch {
			this.dirty = true;
			this.set({ saveState: 'unsaved' });
		}
	}

	update = (fn: (s: Graph) => Graph, opts?: { record?: boolean }) => {
		if (this.snap.status !== 'ready') return;
		const next = fn({ nodes: this.snap.nodes, edges: this.snap.edges });
		this.set({ nodes: next.nodes, edges: next.edges });
		this.markDirty();
		if (opts?.record !== false) this.history.record(next);
	};

	onNodesChange = (changeset: NodeChangeset<CN>) => {
		const changes = changeset.toArray();
		const nodes = detachOrphans(this.snap.nodes, changeset.applyTo(this.snap.nodes));
		const moved = changes.flatMap((c) => (c.type === 'position' ? [c.id] : []));
		// Selection and the first measurement are not edits.
		const edit = changes.some(
			(c) => c.type !== 'select' && !(c.type === 'dimensions' && c.resizing === undefined),
		);
		const commit = changes.some(
			(c) =>
				(c.type === 'position' && c.dragging === false) ||
				c.type === 'remove' ||
				c.type === 'add' ||
				(c.type === 'dimensions' && c.resizing === false),
		);
		this.set({ nodes });
		if (moved.length) this.touchAll(moved);
		if (!edit) return;
		this.markDirty();
		if (commit) this.history.record({ nodes, edges: this.snap.edges });
	};

	onEdgesChange = (changeset: EdgeChangeset<CE>) => {
		const edges = changeset.applyTo(this.snap.edges);
		this.set({ edges });
		if (!changeset.toArray().some((c) => c.type !== 'select')) return;
		this.markDirty();
		this.history.record({ nodes: this.snap.nodes, edges });
	};

	/** Edges made with the connector tool or by dragging between handles. */
	onConnect = (edge: CE) => this.update((s) => ({ ...s, edges: [...s.edges, edge] }));

	touch = (id: string) => this.touchAll([id]);
	private touchAll(ids: string[]) {
		if (!ids.some((id) => this.snap.newIds.has(id))) return;
		const newIds = new Set(this.snap.newIds);
		for (const id of ids) newIds.delete(id);
		this.set({ newIds });
	}

	undo = () => this.step(this.history.undo());
	redo = () => this.step(this.history.redo());
	private step(g: Graph | undefined) {
		if (!g || this.snap.status !== 'ready') return;
		this.set({ nodes: g.nodes, edges: g.edges });
		this.markDirty();
	}
}

/** Deleting a frame keeps its children: they move to absolute positions. */
function detachOrphans(prev: CN[], next: CN[]): CN[] {
	const ids = new Set(next.map((n) => n.id));
	if (!next.some((n) => n.parentId && !ids.has(n.parentId))) return next;
	const old = new Map(prev.map((n) => [n.id, n]));
	return next.map((n) => {
		if (!n.parentId || ids.has(n.parentId)) return n;
		const p = old.get(n.parentId);
		const { parentId: _gone, ...rest } = n;
		return {
			...rest,
			position: { x: n.position.x + (p?.position.x ?? 0), y: n.position.y + (p?.position.y ?? 0) },
		};
	});
}

export function useCanvasDoc(projectId: string, path: string) {
	const ctl = useMemo(() => acquireCanvasDoc(projectId, path), [projectId, path]);
	useEffect(() => {
		ctl.start();
		const onBlur = () => void ctl.saveNow();
		window.addEventListener('blur', onBlur);
		return () => {
			window.removeEventListener('blur', onBlur);
			ctl.dispose();
		};
	}, [ctl]);
	return { ...useSyncExternalStore(ctl.subscribe, ctl.getSnapshot), ctl };
}
