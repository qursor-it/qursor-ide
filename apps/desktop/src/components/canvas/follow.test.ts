import { describe, expect, it } from 'vitest';
import { boundsOf, followViewport, shouldFollow, viewportRect } from './follow';

const view = viewportRect({ x: 0, y: 0, zoom: 1 }, 1000, 600);
const inside = { x: 100, y: 100, width: 160, height: 160 };
const outside = { x: 3000, y: 100, width: 160, height: 160 };

describe('shouldFollow', () => {
	it('visible: new nodes already on screen do not move the board', () => {
		expect(shouldFollow([inside], view, 0, 10_000)).toBe(false);
	});
	it('offscreen_idle: follows', () => {
		expect(shouldFollow([outside], view, 0, 10_000)).toBe(true);
		expect(shouldFollow([outside, inside], view, 0, 10_000)).toBe(false);
	});
	it('recent_interaction: a gesture 1s ago blocks it', () => {
		expect(shouldFollow([outside], view, 9_000, 10_000)).toBe(false);
	});
	it('empty: nothing arrived', () => {
		expect(shouldFollow([], view, 0, 10_000)).toBe(false);
	});
});

describe('bounds', () => {
	it('boundsOf: the box around all rects, null when empty', () => {
		expect(boundsOf([])).toBeNull();
		expect(boundsOf([inside, outside])).toEqual({ x: 100, y: 100, width: 3060, height: 160 });
	});
	it('viewportRect: pan and zoom map back to flow coordinates', () => {
		expect(viewportRect({ x: -200, y: 50, zoom: 2 }, 1000, 600)).toEqual({
			x: 100,
			y: -25,
			width: 500,
			height: 300,
		});
	});
	it('followViewport: keeps zoom when it fits and centres the box', () => {
		const v = followViewport(outside, 1000, 600, 1, 0.1);
		expect(v.zoom).toBe(1);
		expect(viewportRect(v, 1000, 600).x + 500).toBeCloseTo(3080);
	});
	it('followViewport: zooms out to fit but never below minZoom', () => {
		const wide = { x: 0, y: 0, width: 4000, height: 100 };
		expect(followViewport(wide, 1000, 600, 1, 0.1).zoom).toBeCloseTo(1000 / 4128);
		expect(followViewport(wide, 1000, 600, 1, 0.5).zoom).toBe(0.5);
	});
});
