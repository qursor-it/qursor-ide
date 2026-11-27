import type { AppError, ClientMsg, CoreEndpoint, CoreEvent, ServerMsg } from '@factorai/types';
import { invoke } from '@tauri-apps/api/core';
import { formatError, isAppError } from '@lib/errors';
import { isTauri } from '@lib/tauri';
import { decodeFrame, encodeFrame, encodeResize, Op } from './frames';
import { mockCore } from './mock';

// The renderer's side of qursor-core's socket
// (docs/superpowers/specs/2026-11-26-local-core-design.md §3-§4): JSON for
// control, binary frames for terminal bytes in both directions.

export type Payload<E extends CoreEvent['event']> = Extract<CoreEvent, { event: E }>['payload'];

export interface CoreClient {
	request<T>(method: string, params?: unknown): Promise<T>;
	on<E extends CoreEvent['event']>(event: E, cb: (payload: Payload<E>) => void): () => void;
	/** `slot` follows the terminal across a reconnect, which re-attaches it on a
	 *  new one; read it at each `write`/`resize` rather than keeping a copy. */
	attach(
		terminalId: string,
		onOutput: (bytes: Uint8Array) => void,
	): Promise<{ readonly slot: number; detach(): void }>;
	write(slot: number, bytes: Uint8Array): void;
	resize(slot: number, cols: number, rows: number): void;
	setVisible(terminalId: string, visible: boolean): void;
}

const PROTOCOL_VERSION = 1;
const PING_MS = 10_000;
/** Two ping windows with nothing arriving (§3). */
const SILENCE_MS = 2 * PING_MS;
const BACKOFF_MIN_MS = 100;
const BACKOFF_MAX_MS = 2_000;
/** Connections the core closes before its `server_info` — a wrong secret —
 *  before the client gives up rather than retry forever. */
const MAX_REFUSALS = 3;

interface Attachment {
	terminalId: string;
	slot: number;
	onOutput: (bytes: Uint8Array) => void;
}

const lostError = (message: string): AppError => ({ kind: 'Process', message });

/** For a send whose only expected failure is the connection dropping: the
 *  core's state for this client went with it, so there is nothing to undo. */
const unlessLost = (what: string) => (e: unknown) => {
	if (!isAppError(e) || e.kind !== 'Process') console.error(`${what} failed`, e);
};

/** The methods that answer with a slot already attached for the caller. */
const SPAWNS = new Set(['terminal.spawn', 'shell.spawn', 'server.openShell']);

/** One connection to one core, kept alive: reconnects with backoff and puts
 *  every attached terminal back on the new socket. */
export function connectCore(endpoint: () => Promise<CoreEndpoint>): CoreClient {
	const clientId = crypto.randomUUID();
	let ws: WebSocket | undefined;
	let handshaken = false;
	/** Set when retrying cannot help (refused, another protocol): every request
	 *  fails with it from then on. */
	let failed: AppError | undefined;
	let refusals = 0;
	let backoff = BACKOFF_MIN_MS;
	let lastHeard = 0;
	let pinger: ReturnType<typeof setInterval> | undefined;
	let nextRequestId = 1;
	const pending = new Map<
		number,
		{ method: string; resolve: (v: unknown) => void; reject: (e: AppError) => void }
	>();
	/** Slots a spawn on this connection handed back, by terminal, until `attach`
	 *  takes them: no second round trip, so less output falls in the gap. */
	const spawned = new Map<string, number>();
	const listeners = new Map<string, Set<(payload: unknown) => void>>();
	const attachments = new Set<Attachment>();
	const bySlot = new Map<number, Attachment>();
	const shown = new Set<string>();

	// Requests wait here for a handshaken socket. A failed attempt rejects the
	// waiters rather than holding them across retries that may never succeed.
	let ready!: { promise: Promise<void>; resolve: () => void; reject: (e: AppError) => void };
	const newReady = () => {
		let resolve!: () => void;
		let reject!: (e: AppError) => void;
		const promise = new Promise<void>((res, rej) => {
			resolve = res;
			reject = rej;
		});
		// Nobody may be waiting when an attempt fails.
		promise.catch(() => undefined);
		ready = { promise, resolve, reject };
	};
	newReady();

	const sendJson = (msg: ClientMsg) => ws?.send(JSON.stringify(msg));

	const send = <T>(method: string, params: unknown): Promise<T> => {
		const requestId = nextRequestId++;
		return new Promise<T>((resolve, reject) => {
			pending.set(requestId, { method, resolve: (v) => resolve(v as T), reject });
			// `params` is required on the wire, so an absent one is `null`.
			sendJson({ type: 'request', requestId, method, params: params ?? null });
		});
	};

	const request = async <T>(method: string, params?: unknown): Promise<T> => {
		if (failed) throw failed;
		if (!handshaken) await ready.promise;
		return send<T>(method, params);
	};

	const detachSlot = (slot: number) =>
		void send('terminal.detach', { slot }).catch(unlessLost('terminal.detach'));

	const reattach = (att: Attachment) => {
		send<{ slot: number }>('terminal.attach', { terminalId: att.terminalId }).then(
			({ slot }) => {
				// Detached while the answer was on its way: give the slot back.
				if (!attachments.has(att)) return detachSlot(slot);
				att.slot = slot;
				bySlot.set(slot, att);
			},
			(e: unknown) => {
				if (!isAppError(e)) return console.error('terminal.attach (re-attach) failed', e);
				// The terminal ended while we were away: nothing left to stream.
				if (e.kind === 'NotFound') attachments.delete(att);
				// Lost again: the next handshake re-attaches it.
				else if (e.kind !== 'Process') console.error('terminal.attach (re-attach) failed', e);
			},
		);
	};

	const onServerInfo = (version: number) => {
		if (version !== PROTOCOL_VERSION) {
			return fail(
				ws,
				lostError(`the core speaks protocol ${version}; this window speaks ${PROTOCOL_VERSION}`),
			);
		}
		handshaken = true;
		refusals = 0;
		backoff = BACKOFF_MIN_MS;
		pinger = setInterval(() => {
			if (Date.now() - lastHeard >= SILENCE_MS) lost(ws, 'the core stopped answering');
			else sendJson({ type: 'ping' });
		}, PING_MS);
		for (const att of attachments) reattach(att);
		for (const terminalId of shown) {
			void send('terminal.visibility', { terminalId, visible: true }).catch(
				unlessLost('terminal.visibility'),
			);
		}
		ready.resolve();
	};

	const onText = (msg: ServerMsg) => {
		switch (msg.type) {
			case 'server_info':
				return onServerInfo(msg.protocolVersion);
			case 'pong':
				return;
			case 'response':
			case 'rpc_error': {
				const call = pending.get(msg.requestId);
				pending.delete(msg.requestId);
				if (msg.type === 'rpc_error') return call?.reject(msg.error);
				if (call && SPAWNS.has(call.method)) {
					const { id, slot } = msg.result as { id: string; slot: number };
					spawned.set(id, slot);
				}
				call?.resolve(msg.result);
				return;
			}
			case 'event':
				for (const cb of listeners.get(msg.event) ?? []) cb(msg.payload);
		}
	};

	/** Drop this socket and everything that lived on it; false if it is not
	 *  the current one. */
	const teardown = (sock: WebSocket | undefined, error: AppError): boolean => {
		if (!sock || sock !== ws) return false;
		sock.onopen = sock.onmessage = sock.onclose = sock.onerror = null;
		sock.close();
		ws = undefined;
		handshaken = false;
		clearInterval(pinger);
		// Slots are per connection and the next one numbers from 1 again: an old
		// number may name another terminal there, so nothing may use it.
		bySlot.clear();
		spawned.clear();
		for (const att of attachments) att.slot = 0;
		for (const call of pending.values()) call.reject(error);
		pending.clear();
		return true;
	};

	/** Drop this socket and try again. Also what liveness does: a dead TCP
	 *  connection may take minutes to report its own close. */
	const lost = (sock: WebSocket | undefined, why: string) => {
		if (teardown(sock, lostError(why))) retry(why);
	};

	/** Drop this socket and stop: no retry would succeed. */
	const fail = (sock: WebSocket | undefined, error: AppError) => {
		if (!teardown(sock, error)) return;
		failed = error;
		console.error(`qursor-core: ${error.message}`);
		ready.reject(error);
	};

	/** Closed after it opened, before any `server_info`: the core turned us away. */
	const refused = (sock: WebSocket) => {
		refusals += 1;
		if (refusals < MAX_REFUSALS) return lost(sock, 'the core closed the connection at hello');
		fail(sock, lostError('the core refused this window: its secret is not the one it was given'));
	};

	const retry = (why: string) => {
		ready.reject(lostError(why));
		newReady();
		setTimeout(open, backoff);
		backoff = Math.min(backoff * 2, BACKOFF_MAX_MS);
	};

	async function open() {
		let ep: CoreEndpoint;
		try {
			ep = await endpoint();
		} catch (e) {
			return retry(`the core is not reachable: ${formatError(e)}`);
		}
		let sock: WebSocket;
		try {
			sock = new WebSocket(ep.url);
		} catch (e) {
			return retry(`could not open a connection to the core: ${formatError(e)}`);
		}
		ws = sock;
		sock.binaryType = 'arraybuffer';
		let opened = false;
		sock.onopen = () => {
			opened = true;
			sendJson({
				type: 'hello',
				secret: ep.secret,
				clientId,
				clientType: 'renderer',
				protocolVersion: PROTOCOL_VERSION,
				appVersion: __APP_VERSION__,
				capabilities: [],
			});
		};
		sock.onmessage = (e: MessageEvent) => {
			lastHeard = Date.now();
			if (typeof e.data === 'string') return onText(JSON.parse(e.data) as ServerMsg);
			// The view goes straight to xterm: no copy, no base64 (§4).
			const frame = decodeFrame(e.data as ArrayBuffer);
			if (frame.op === Op.Output) bySlot.get(frame.slot)?.onOutput(frame.payload);
		};
		// A browser may report a refusal as an error before the close.
		const dropped = (why: string) => (opened && !handshaken ? refused(sock) : lost(sock, why));
		sock.onclose = () => dropped('the connection to the core closed');
		sock.onerror = () => dropped('the connection to the core failed');
	}

	void open();

	return {
		request,
		on(event, cb) {
			const fn = (payload: unknown) => cb(payload as Payload<typeof event>);
			const set = listeners.get(event) ?? new Set();
			set.add(fn);
			listeners.set(event, set);
			return () => set.delete(fn);
		},
		async attach(terminalId, onOutput) {
			let slot = spawned.get(terminalId);
			spawned.delete(terminalId);
			slot ??= (await request<{ slot: number }>('terminal.attach', { terminalId })).slot;
			const att: Attachment = { terminalId, slot, onOutput };
			attachments.add(att);
			bySlot.set(slot, att);
			return {
				get slot() {
					return att.slot;
				},
				detach() {
					attachments.delete(att);
					if (bySlot.get(att.slot) !== att) return;
					bySlot.delete(att.slot);
					detachSlot(att.slot);
				},
			};
		},
		// Input for a slot this connection has not attached (no socket, or a
		// re-attach still on its way) is dropped, never replayed: a keystroke sent
		// twice, or to another terminal, is worse than one lost.
		write(slot, bytes) {
			if (handshaken && bySlot.has(slot)) ws?.send(encodeFrame(Op.Input, slot, bytes));
		},
		resize(slot, cols, rows) {
			if (handshaken && bySlot.has(slot)) ws?.send(encodeResize(slot, cols, rows));
		},
		setVisible(terminalId, visible) {
			if (visible === shown.has(terminalId)) return;
			if (visible) shown.add(terminalId);
			else shown.delete(terminalId);
			// Not connected: `onServerInfo` sends every shown terminal.
			if (handshaken) {
				void send('terminal.visibility', { terminalId, visible }).catch(
					unlessLost('terminal.visibility'),
				);
			}
		},
	};
}

const clients = new Map<string, CoreClient>();

/** One client per host; only the local core exists today (design §6 adds more). */
export function coreClient(hostId = 'local'): CoreClient {
	let client = clients.get(hostId);
	if (!client) {
		if (isTauri()) {
			client = connectCore(() => invoke<CoreEndpoint>('core_endpoint'));
			// The core decides that a session wants you; only the app can show it (F32).
			client.on('attention', (notice) => {
				invoke('attention_show', { notice }).catch((e: unknown) =>
					console.error('attention_show failed', e),
				);
			});
		} else {
			client = mockCore();
		}
		clients.set(hostId, client);
	}
	return client;
}
