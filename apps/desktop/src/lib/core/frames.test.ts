import { describe, expect, it } from 'vitest';
import { decodeFrame, encodeFrame, encodeResize, Op } from './frames';

const bufferOf = (bytes: Uint8Array): ArrayBuffer => bytes.slice().buffer;

describe('frames', () => {
	it('roundtrip_output', () => {
		const data = new Uint8Array(32 * 1024).map((_, i) => (Math.imul(i, 2654435761) >>> 24) & 0xff);
		const frame = decodeFrame(bufferOf(encodeFrame(Op.Output, 513, data)));
		expect([frame.op, frame.slot]).toEqual([Op.Output, 513]);
		expect(frame.payload).toEqual(data);
	});

	it('resize_payload_is_four_bytes', () => {
		const frame = encodeResize(7, 120, 40);
		expect([...frame]).toEqual([3, 0, 7, 0, 120, 0, 40]);
		expect(decodeFrame(bufferOf(frame)).payload.length).toBe(4);
		expect(() => decodeFrame(bufferOf(new Uint8Array([3, 0, 7, 0, 120])))).toThrow('four bytes');
	});

	it('rejects_short_frame', () => {
		expect(() => decodeFrame(bufferOf(new Uint8Array([1, 0])))).toThrow('shorter');
	});

	it('rejects_unknown_op', () => {
		expect(() => decodeFrame(bufferOf(new Uint8Array([9, 0, 0])))).toThrow('unknown opcode 9');
		expect(() => decodeFrame(bufferOf(new Uint8Array([4, 0, 0])))).toThrow('unknown opcode 4');
	});

	it('decode_is_a_view', () => {
		const buf = bufferOf(new Uint8Array([1, 0, 1, 42]));
		expect(decodeFrame(buf).payload.buffer).toBe(buf);
	});

	// Same vector as `shared_vector` in crates/qursor-core/src/protocol/frame.rs.
	it('shared_vector', () => {
		const { op, slot, payload } = decodeFrame(bufferOf(new Uint8Array([1, 2, 1, 104, 105])));
		expect([op, slot, new TextDecoder().decode(payload)]).toEqual([Op.Output, 513, 'hi']);
	});
});
