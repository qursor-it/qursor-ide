import { describe, expect, it } from 'vitest';
import { readPoints, strokeToNode } from './ink';

describe('strokeToNode', () => {
	it('stroke_bbox: the bbox is the node, points are relative', () => {
		const n = strokeToNode(
			[
				[10, 20, 0.5],
				[30, 60, 0.5],
			],
			'a',
		);
		expect([n.x, n.y, n.width, n.height]).toEqual([10, 20, 20, 40]);
		expect((n['x-points'] as number[][])[0]).toEqual([0, 0, 0.5]);
		expect(n['x-kind']).toBe('ink');
		expect(n.type).toBe('text');
	});

	it('tiny_stroke_min_size: a single point is 8x8', () => {
		const n = strokeToNode([[5, 5, 0.5]], 'b');
		expect([n.width, n.height]).toEqual([8, 8]);
	});
});

describe('readPoints', () => {
	it('drops malformed entries and defaults pressure', () => {
		expect(readPoints([[1, 2], 'x', [3, 'y'], [4, 5, 0.9]])).toEqual([
			[1, 2, 0.5],
			[4, 5, 0.9],
		]);
		expect(readPoints(undefined)).toEqual([]);
	});
});
