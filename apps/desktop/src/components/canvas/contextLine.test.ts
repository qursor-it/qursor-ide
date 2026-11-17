import { describe, expect, it } from 'vitest';
import { buildContextLine, buildMessage } from './contextLine';

describe('contextLine', () => {
	it('with_selection', () => {
		expect(buildContextLine(String.raw`E:\p\canvas\a.canvas`, ['a1', 'b7'])).toBe(
			String.raw`[canvas: E:\p\canvas\a.canvas · selected nodes: a1, b7 · JSON Canvas 1.0; mark nodes you add with "x-author":"agent"; keep fields you don't know]`,
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
});
