import { describe, expect, it } from 'vitest';
import { fitFrame } from './frame';

describe('fitFrame', () => {
	it('wraps the children bbox with padding and keeps their absolute positions', () => {
		const frame = { x: 100, y: 200, w: 500, h: 400 };
		const kids = [
			{ id: 'a', x: 50, y: 60, w: 100, h: 40 },
			{ id: 'b', x: 200, y: 150, w: 80, h: 80 },
		];
		const r = fitFrame(frame, kids, 24);
		expect(r?.frame).toEqual({ x: 126, y: 236, w: 230 + 48, h: 170 + 48 });
		expect(r?.children).toEqual([
			{ id: 'a', x: 24, y: 24 },
			{ id: 'b', x: 174, y: 114 },
		]);
		// absolute = frame origin + relative, before and after
		expect((r?.frame.x ?? 0) + 24).toBe(frame.x + 50);
	});

	it('is a no-op without children', () => {
		expect(fitFrame({ x: 0, y: 0, w: 10, h: 10 }, [], 24)).toBeNull();
	});
});
