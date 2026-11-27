import type { ClientMsg, ServerMsg } from '@factorai/types';
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
});
