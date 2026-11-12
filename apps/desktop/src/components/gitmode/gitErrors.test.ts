import type { GitOp, GitOpResult } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import { explainGitError } from './gitErrors';

const fail = (stderr: string, stdout = ''): GitOpResult => ({
	command: 'git x',
	ok: false,
	stdout,
	stderr,
});
const push: GitOp = { kind: 'push', setUpstream: false };
const kinds = (op: GitOp, r: GitOpResult) => explainGitError(op, r)?.actions.map((a) => a.kind);

describe('explainGitError', () => {
	it('push_rejected_offers_pull_then_push', () => {
		const r = fail(
			"! [rejected]        main -> main (fetch first)\nerror: failed to push some refs to 'origin'",
		);
		const info = explainGitError(push, r);
		expect(info?.title).toMatch(/^Push rejected/);
		expect(kinds(push, r)).toEqual(['pullThenPush', 'askAgent', 'copy']);
	});

	it('local_changes_offer_stash_and_retry', () => {
		const r = fail(
			'error: Your local changes to the following files would be overwritten by merge:\n\ta.ts',
		);
		expect(explainGitError({ kind: 'pull' }, r)?.title).toBe('Local changes are in the way');
		expect(kinds({ kind: 'pull' }, r)).toEqual(['stashAndRetry', 'copy']);
	});

	it('no_upstream_offers_set_upstream_only_for_push_and_pull', () => {
		const r = fail(
			'fatal: The current branch feat has no upstream branch.\nTo push the current branch and set the remote as upstream, use',
		);
		expect(explainGitError(push, r)?.title).toBe('This branch has no upstream');
		expect(kinds(push, r)).toEqual(['pushSetUpstream', 'copy']);
		expect(kinds({ kind: 'fetch' }, r)).toEqual(['copy']);
	});

	it('lock_offers_retry', () => {
		const r = fail("fatal: Unable to create '/r/.git/index.lock': File exists.");
		expect(explainGitError({ kind: 'stage', paths: [] }, r)?.title).toMatch(/locked/);
		expect(kinds(push, r)).toEqual(['retry', 'copy']);
	});

	it('auth_and_network_offer_retry', () => {
		for (const s of [
			"fatal: Authentication failed for 'https://x/y.git'",
			'fatal: Could not resolve host: github.com',
			'git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.',
		]) {
			expect(explainGitError(push, fail(s))?.title).toBe('Could not reach the remote');
			expect(kinds(push, fail(s))).toEqual(['retry', 'copy']);
		}
	});

	it('git_not_found_offers_only_copy', () => {
		const r = fail('git not found on PATH');
		expect(explainGitError({ kind: 'fetch' }, r)?.title).toBe(
			'git is not installed or not on PATH',
		);
		expect(kinds({ kind: 'fetch' }, r)).toEqual(['copy']);
	});

	it('anything_else_is_titled_by_the_op', () => {
		const r = fail('fatal: something odd');
		expect(explainGitError(push, r)?.title).toBe('Push failed');
		expect(explainGitError({ kind: 'commit', message: 'm', amend: false }, r)?.title).toBe(
			'Commit failed',
		);
		expect(kinds(push, r)).toEqual(['retry', 'askAgent', 'copy']);
	});

	it('conflicts_never_open_a_dialog', () => {
		for (const s of [
			'CONFLICT (content): Merge conflict in a.ts',
			'Automatic merge failed; fix conflicts and then commit the result.',
			'error: could not apply 1234abc... msg',
		])
			expect(explainGitError({ kind: 'merge', branch: 'x' }, fail('', s))).toBeNull();
	});

	it('cherry_pick_and_revert_conflicts_are_left_to_the_banner', () => {
		const f = fail('', 'CONFLICT (content): Merge conflict in a.ts');
		for (const op of [
			{ kind: 'cherryPick', sha: 'abc1234' },
			{ kind: 'revert', sha: 'abc1234' },
			{ kind: 'cherryPickContinue' },
			{ kind: 'revertContinue' },
		] as const)
			expect(explainGitError(op, f)).toBeNull();
	});

	it('a_conflicted_stash_pop_still_opens_a_dialog', () => {
		const info = explainGitError(
			{ kind: 'stashPop', index: 0 },
			fail('', 'CONFLICT (content): Merge conflict in a.ts'),
		);
		expect(info).not.toBeNull();
	});

	it('ok_results_never_open_a_dialog', () => {
		expect(
			explainGitError(push, { command: 'git push', ok: true, stdout: '', stderr: '' }),
		).toBeNull();
	});
});
