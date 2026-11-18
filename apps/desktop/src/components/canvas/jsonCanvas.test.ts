import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { type CanvasDoc, newNodeIds, parseCanvas, serializeCanvas } from './jsonCanvas';

const SAMPLE = JSON.stringify({
	title: 'T',
	meta: {},
	nodes: [
		{ id: 'g', type: 'group', x: 0, y: 0, width: 400, height: 400, label: 'Frame' },
		{
			id: 'a',
			type: 'text',
			x: 50,
			y: 60,
			width: 100,
			height: 80,
			text: 'hi',
			'x-kind': 'sticky',
			'x-foo': 1,
		},
		{ id: 'b', type: 'text', x: 600, y: 10, width: 100, height: 80, text: 'plain' },
	],
	edges: [{ id: 'e', fromNode: 'a', toNode: 'b', bar: true }],
});

function parse(text: string) {
	const r = parseCanvas(text);
	if (!r.ok) throw new Error(r.error);
	return r;
}
const roundtrip = (text: string): string => {
	const r = parse(text);
	return serializeCanvas(r.doc, r.nodes, r.edges);
};

describe('jsonCanvas', () => {
	it('roundtrip_preserves_unknown', () => {
		expect(JSON.parse(roundtrip(SAMPLE))).toEqual(JSON.parse(SAMPLE));
	});

	it('roundtrip_is_stable', () => {
		const once = roundtrip(SAMPLE);
		expect(roundtrip(once)).toBe(once);
		expect(once.endsWith('}\n')).toBe(true);
		expect(once).toContain('\n  "title"');
	});

	it('maps_kinds', () => {
		const r = parse(SAMPLE);
		const by = (id: string) => r.nodes.find((n) => n.id === id);
		expect(by('a')?.type).toBe('sticky');
		expect(by('b')?.type).toBe('text');
		expect(by('g')?.type).toBe('group');
		expect(by('a')?.parentId).toBe('g');
		expect(by('a')?.position).toEqual({ x: 50, y: 60 });
		expect(by('b')?.parentId).toBeUndefined();
	});

	it('makes child positions relative and absolute again', () => {
		const text = JSON.stringify({
			nodes: [
				{ id: 'g', type: 'group', x: 100, y: 200, width: 400, height: 400 },
				{ id: 'c', type: 'text', x: 150, y: 260, width: 50, height: 50 },
			],
			edges: [],
		});
		const r = parse(text);
		expect(r.nodes.find((n) => n.id === 'c')?.position).toEqual({ x: 50, y: 60 });
		const out = JSON.parse(roundtrip(text));
		expect(out.nodes[1]).toMatchObject({ id: 'c', x: 150, y: 260 });
	});

	it('invalid_json', () => {
		expect(parseCanvas('{').ok).toBe(false);
		expect(parseCanvas('{"nodes":5}').ok).toBe(false);
		expect(parseCanvas('[]').ok).toBe(false);
		expect(parseCanvas('{"nodes":[{"type":"text"}]}').ok).toBe(false);
	});

	it('missing_arrays_default', () => {
		const r = parseCanvas('{"title":"t"}');
		expect(r.ok && r.nodes).toEqual([]);
		expect(r.ok && r.edges).toEqual([]);
	});

	it('new_node_ids', () => {
		const doc = (...ids: string[]): CanvasDoc => ({
			nodes: ids.map((id) => ({ id, type: 'text', x: 0, y: 0, width: 1, height: 1 })),
			edges: [],
		});
		expect(newNodeIds(doc('a'), doc('a', 'b'))).toEqual(new Set(['b']));
		expect(newNodeIds(null, doc('a')).size).toBe(0);
	});
});

// The Rust side (`services/canvas_tools.rs`) asserts the same table on the same file.
describe('containment parity with the agent tools', () => {
	it('assigns the parents the Rust test expects', () => {
		const text = readFileSync(
			new URL('./__fixtures__/containment.canvas', import.meta.url),
			'utf8',
		);
		const parsed = parseCanvas(text);
		if (!parsed.ok) throw new Error(parsed.error);
		const parents = Object.fromEntries(parsed.nodes.map((n) => [n.id, n.parentId ?? null]));
		expect(parents).toEqual({
			'g-big': null,
			'g-small': 'g-big',
			'n-in-small': 'g-small',
			'n-in-big': 'g-big',
			'n-in-big-2': 'g-big',
			'n-straddle': null,
			'n-loose': null,
		});
	});
});
