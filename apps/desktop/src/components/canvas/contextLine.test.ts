import { describe, expect, it } from 'vitest';
import { buildContextLine, buildMessage, buildPrimer } from './contextLine';

describe('contextLine', () => {
	it('with_selection', () => {
		expect(buildContextLine(String.raw`E:\p\canvas\a.canvas`, ['a1', 'b7'])).toBe(
			String.raw`[canvas: E:\p\canvas\a.canvas · selected nodes: a1, b7 · JSON Canvas 1.0; mark nodes you add with "x-author":"agent"; use colour presets "1"-"6", not hex; prefer the canvas* tools over reading or rewriting the whole file; keep fields you don't know]`,
		);
	});
	it('no_selection', () => {
		expect(buildContextLine('/p/a.canvas', [])).toContain('selected nodes: none');
	});
	it('message is the line, a newline, then the text', () => {
		expect(buildMessage('/p/a.canvas', [], 'hi')).toBe(
			`${buildContextLine('/p/a.canvas', [])}\nhi`,
		);
	});
	it('primer starts with the context line and names the file', () => {
		const p = buildPrimer('/p/a.canvas');
		expect(p.startsWith(buildContextLine('/p/a.canvas', []))).toBe(true);
		expect(p).toContain('shared whiteboard');
	});
});
