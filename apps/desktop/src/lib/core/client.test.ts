import type { AppError, ClientMsg, ServerMsg } from '@factorai/types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { connectCore } from './client';
import { encodeFrame, Op } from './frames';

/** Just enough of a browser WebSocket for the client: handlers, `send`,
 *  `close`, and the test's side of the wire. */
class FakeWebSocket {
	static all: FakeWebSocket[] = [];
	binaryType = 'blob';
	sent: Array<string | Uint8Array> = [];
	closed = false;
	onopen: (() => void) | null = null;
	onmessage: ((e: { data: unknown }) => void) | null = null;
	onclose: (() => void) | null = null;
	onerror: (() => void) | null = null;

	constructor(readonly url: string) {
		FakeWebSocket.all.push(this);
	}
	send(data: string | Uint8Array) {
		this.sent.push(data);
	}
	close() {
		this.closed = true;
	}

	receive(msg: ServerMsg) {
		this.onmessage?.({ data: JSON.stringify(msg) });
	}
	receiveBinary(bytes: Uint8Array) {
		this.onmessage?.({ data: bytes.slice().buffer });
	}
	json(): ClientMsg[] {
		return this.sent.filter((d) => typeof d === 'string').map((d) => JSON.parse(d) as ClientMsg);
	}
	binary(): Uint8Array[] {
		return this.sent.filter((d) => typeof d !== 'string');
	}
	/** Answer the last request for `method`. */
	answer(method: string, result: unknown) {
		const req = this.json()
			.filter((m) => m.type === 'request' && m.method === method)
			.pop();
		if (req?.type !== 'request') throw new Error(`no ${method} request was sent`);
		this.receive({ type: 'response', requestId: req.requestId, result });
	}
	/** Fail the last request for `method`. */
	fail(method: string, error: AppError) {
		const req = this.json()
			.filter((m) => m.type === 'request' && m.method === method)
			.pop();
		if (req?.type !== 'request') throw new Error(`no ${method} request was sent`);
		this.receive({ type: 'rpc_error', requestId: req.requestId, error });
	}
	requests(method: string) {
		return this.json().filter((m) => m.type === 'request' && m.method === method);
	}
}

const SERVER_INFO: ServerMsg = {
	type: 'server_info',
	hostId: 'host-1',
	version: '0.0.0',
	protocolVersion: 1,
	features: [],
};

const flush = () => vi.advanceTimersByTimeAsync(0);
const latest = () => FakeWebSocket.all[FakeWebSocket.all.length - 1];

async function connected() {
	const client = connectCore(async () => ({
		url: 'ws://127.0.0.1:1',
		secret: 's3cret',
		hostId: 'h',
	}));
	await flush();
	const ws = latest();
	ws.onopen?.();
	ws.receive(SERVER_INFO);
	await flush();
	return { client, ws };
}

async function attach(
	client: ReturnType<typeof connectCore>,
	ws: FakeWebSocket,
	terminalId: string,
	slot: number,
) {
	const out: Uint8Array[] = [];
	const pending = client.attach(terminalId, (bytes) => out.push(bytes.slice()));
	await flush();
	ws.answer('terminal.attach', { slot });
	const handle = await pending;
	return { handle, out };
}

/** Close `ws`, let the client dial again, and finish the handshake. */
async function reconnect(ws: FakeWebSocket) {
	ws.onclose?.();
	await vi.advanceTimersByTimeAsync(2_000);
	const next = latest();
	next.onopen?.();
	next.receive(SERVER_INFO);
	await flush();
	return next;
}

beforeEach(() => {
	vi.useFakeTimers();
	FakeWebSocket.all = [];
	vi.stubGlobal('WebSocket', FakeWebSocket);
});

afterEach(() => {
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

describe('core client', () => {
	it('sends_hello_first', async () => {
		connectCore(async () => ({ url: 'ws://127.0.0.1:1', secret: 's3cret', hostId: 'h' }));
		await flush();
		const ws = latest();
		expect(ws.url).toBe('ws://127.0.0.1:1');
		expect(ws.binaryType).toBe('arraybuffer');
		ws.onopen?.();
		expect(ws.json()[0]).toMatchObject({
			type: 'hello',
			secret: 's3cret',
			clientType: 'renderer',
			protocolVersion: 1,
		});
		expect(ws.sent).toHaveLength(1);
	});

	it('routes_output_by_slot', async () => {
		const { client, ws } = await connected();
		const one = await attach(client, ws, 't1', 1);
		const two = await attach(client, ws, 't2', 2);
		ws.receiveBinary(encodeFrame(Op.Output, 2, new Uint8Array([104, 105])));
		expect(one.out).toEqual([]);
		expect(two.out).toEqual([new Uint8Array([104, 105])]);
	});

	it('reattaches_after_reconnect', async () => {
		const { client, ws } = await connected();
		const { handle, out } = await attach(client, ws, 't1', 1);
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(100);
		const next = latest();
		expect(next).not.toBe(ws);
		next.onopen?.();
		expect(next.json()[0]).toMatchObject({ type: 'hello', secret: 's3cret' });
		next.receive(SERVER_INFO);
		await flush();
		expect(next.json()[1]).toMatchObject({
			type: 'request',
			method: 'terminal.attach',
			params: { terminalId: 't1' },
		});
		next.answer('terminal.attach', { slot: 7 });
		await flush();
		expect(handle.slot).toBe(7);
		next.receiveBinary(encodeFrame(Op.Output, 7, new Uint8Array([1, 2, 3])));
		expect(out).toEqual([new Uint8Array([1, 2, 3])]);
	});

	it('fails_in_flight_requests_on_close', async () => {
		const { client, ws } = await connected();
		const pending = client.request('core.counts');
		await flush();
		ws.onclose?.();
		await expect(pending).rejects.toMatchObject({ kind: 'Process' });
	});

	it('no_duplicate_input_after_reconnect', async () => {
		const { client, ws } = await connected();
		const { handle } = await attach(client, ws, 't1', 1);
		ws.onclose?.();
		await vi.advanceTimersByTimeAsync(100);
		const next = latest();
		next.onopen?.();
		next.receive(SERVER_INFO);
		await flush();
		next.answer('terminal.attach', { slot: 4 });
		await flush();
		client.write(handle.slot, new TextEncoder().encode('ls\r'));
		expect(ws.binary()).toEqual([]);
		expect(next.binary()).toEqual([encodeFrame(Op.Input, 4, new TextEncoder().encode('ls\r'))]);
	});

	it('liveness_closes_after_two_silent_windows', async () => {
		const { ws } = await connected();
		await vi.advanceTimersByTimeAsync(10_000);
		expect(ws.json().pop()).toEqual({ type: 'ping' });
		expect(ws.closed).toBe(false);
		await vi.advanceTimersByTimeAsync(10_000);
		expect(ws.closed).toBe(true);
	});

	it('a_pong_keeps_the_socket_alive', async () => {
		const { ws } = await connected();
		await vi.advanceTimersByTimeAsync(10_000);
		ws.receive({ type: 'pong' });
		await vi.advanceTimersByTimeAsync(10_000);
		expect(ws.closed).toBe(false);
	});
	it('no_input_on_a_stale_slot_before_reattach', async () => {
		const { client, ws } = await connected();
		const { handle } = await attach(client, ws, 't1', 1);
		const next = await reconnect(ws);
		// The re-attach has not been answered: slot 1 may be another terminal here.
		client.write(handle.slot, new Uint8Array([1]));
		client.write(1, new Uint8Array([1]));
		client.resize(1, 80, 24);
		expect(next.binary()).toEqual([]);
	});

	it('reattach_lost_again_keeps_the_terminal', async () => {
		const { client, ws } = await connected();
		const { handle, out } = await attach(client, ws, 't1', 1);
		const second = await reconnect(ws);
		expect(second.requests('terminal.attach')).toHaveLength(1);
		const third = await reconnect(second);
		expect(third.requests('terminal.attach')).toHaveLength(1);
		third.answer('terminal.attach', { slot: 2 });
		await flush();
		expect(handle.slot).toBe(2);
		third.receiveBinary(encodeFrame(Op.Output, 2, new Uint8Array([9])));
		expect(out).toEqual([new Uint8Array([9])]);
	});

	it('reattach_not_found_drops_the_terminal', async () => {
		const { client, ws } = await connected();
		await attach(client, ws, 't1', 1);
		const second = await reconnect(ws);
		second.fail('terminal.attach', { kind: 'NotFound', message: 'terminal t1' });
		await flush();
		const third = await reconnect(second);
		expect(third.requests('terminal.attach')).toEqual([]);
	});

	it('detach_during_reattach_frees_the_new_slot', async () => {
		const { client, ws } = await connected();
		const { handle } = await attach(client, ws, 't1', 1);
		const next = await reconnect(ws);
		handle.detach();
		next.answer('terminal.attach', { slot: 9 });
		await flush();
		expect(next.requests('terminal.detach')).toMatchObject([{ params: { slot: 9 } }]);
	});

	it('attach_uses_the_slot_a_spawn_returned', async () => {
		const { client, ws } = await connected();
		const spawn = client.request<{ id: string }>('terminal.spawn', { sessionId: 's' });
		await flush();
		ws.answer('terminal.spawn', { id: 't9', agent: null, slot: 3 });
		await spawn;
		const out: Uint8Array[] = [];
		const handle = await client.attach('t9', (b) => out.push(b.slice()));
		expect(ws.requests('terminal.attach')).toEqual([]);
		expect(handle.slot).toBe(3);
		ws.receiveBinary(encodeFrame(Op.Output, 3, new Uint8Array([7])));
		expect(out).toEqual([new Uint8Array([7])]);
	});
});
