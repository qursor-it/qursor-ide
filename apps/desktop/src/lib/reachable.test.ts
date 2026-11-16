import { describe, expect, it } from 'vitest';
import { reachableFrom } from './reachable';

describe('reachableFrom', () => {
	it('walks parents from HEAD and leaves other branches out', () => {
		const commits = [
			{ sha: 'up', parents: ['head'] },
			{ sha: 'other', parents: ['base'] },
			{ sha: 'head', parents: ['mid'] },
			{ sha: 'mid', parents: ['base'] },
			{ sha: 'base', parents: ['older-than-the-page'] },
		];
		expect([...reachableFrom('head', commits)].sort()).toEqual(['base', 'head', 'mid']);
		expect(reachableFrom(null, commits).size).toBe(0);
	});
});
