import { describe, expect, it } from 'vitest';
import { branchTree } from './branchTree';

describe('branchTree', () => {
	it('puts prefixed branches in folders, folders first', () => {
		expect(
			branchTree(['main', 'design/redesign', 'design/anti-slop', 'feat/a/b', 'develop']),
		).toEqual([
			{
				kind: 'folder',
				name: 'design',
				path: 'design',
				children: [
					{ kind: 'branch', name: 'anti-slop', full: 'design/anti-slop' },
					{ kind: 'branch', name: 'redesign', full: 'design/redesign' },
				],
			},
			{
				kind: 'folder',
				name: 'feat',
				path: 'feat',
				children: [
					{
						kind: 'folder',
						name: 'a',
						path: 'feat/a',
						children: [{ kind: 'branch', name: 'b', full: 'feat/a/b' }],
					},
				],
			},
			{ kind: 'branch', name: 'develop', full: 'develop' },
			{ kind: 'branch', name: 'main', full: 'main' },
		]);
	});
});
