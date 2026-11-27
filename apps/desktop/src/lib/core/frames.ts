import type { Op as OpCode } from '@factorai/types';

// Binary frames: `opcode u8 | slot u16 BE | payload`
// (docs/superpowers/specs/2026-11-26-local-core-design.md §3). Mirrors
// `qursor_core::protocol::frame`.

const HEADER = 3;

export const Op = { Output: 1, Input: 2, Resize: 3 } as const satisfies Record<string, OpCode>;

export function encodeFrame(
	op: OpCode,
	slot: number,
	payload: Uint8Array,
): Uint8Array<ArrayBuffer> {
	const out = new Uint8Array(HEADER + payload.length);
	new DataView(out.buffer).setUint16(1, slot);
	out[0] = op;
	out.set(payload, HEADER);
	return out;
}

/** `payload` is a view into `buf`, not a copy. */
export function decodeFrame(buf: ArrayBuffer): { op: OpCode; slot: number; payload: Uint8Array } {
	if (buf.byteLength < HEADER) throw new Error('frame shorter than its header');
	const view = new DataView(buf);
	const op = view.getUint8(0);
	if (op !== Op.Output && op !== Op.Input && op !== Op.Resize) {
		throw new Error(`unknown opcode ${op}`);
	}
	const payload = new Uint8Array(buf, HEADER);
	if (op === Op.Resize && payload.length !== 4) throw new Error('resize payload is not four bytes');
	return { op, slot: view.getUint16(1), payload };
}

export function encodeResize(slot: number, cols: number, rows: number): Uint8Array<ArrayBuffer> {
	const payload = new Uint8Array(4);
	const view = new DataView(payload.buffer);
	view.setUint16(0, cols);
	view.setUint16(2, rows);
	return encodeFrame(Op.Resize, slot, payload);
}
