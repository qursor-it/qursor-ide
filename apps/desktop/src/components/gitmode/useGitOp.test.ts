import type { GitOpResult } from '@factorai/types';
import { cmd } from '@lib/tauri';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { runGitOp, useGitUi } from './useGitOp';

vi.mock('@lib/tauri', () => ({ cmd: { gitOp: vi.fn() } }));

const qc = { invalidateQueries: vi.fn() };
const ok: GitOpResult = { command: 'git fetch', ok: true, stdout: '', stderr: '' };

beforeEach(() => {
	useGitUi.setState({ logs: {}, confirm: null });
});

describe('runGitOp', () => {
	it('a_second_run_while_running_is_ignored', async () => {
		let finish: (r: GitOpResult) => void = () => {};
		vi.mocked(cmd.gitOp).mockReturnValueOnce(new Promise((r) => (finish = r)));
		const first = runGitOp('/r', { kind: 'fetch' }, undefined, qc);
		const second = await runGitOp('/r', { kind: 'pull' }, undefined, qc);
		expect(second).toBeNull();
		finish(ok);
		await first;
		expect(cmd.gitOp).toHaveBeenCalledTimes(1);
		// Settled, so the next one goes through.
		vi.mocked(cmd.gitOp).mockResolvedValueOnce(ok);
		expect(await runGitOp('/r', { kind: 'pull' }, undefined, qc)).toEqual(ok);
	});

	it('failed_result_is_logged_not_thrown', async () => {
		const failed = { command: 'git merge x', ok: false, stdout: '', stderr: 'CONFLICT' };
		vi.mocked(cmd.gitOp).mockResolvedValueOnce(failed);
		await expect(runGitOp('/a', { kind: 'merge', branch: 'x' }, undefined, qc)).resolves.toEqual(
			failed,
		);
		expect(useGitUi.getState().logs['/a']).toEqual(failed);

		vi.mocked(cmd.gitOp).mockRejectedValueOnce({
			kind: 'NotFound',
			message: 'git not found on PATH',
		});
		const r = await runGitOp('/b', { kind: 'fetch' }, undefined, qc);
		expect(r?.ok).toBe(false);
		expect(useGitUi.getState().logs['/b']?.stderr).toBe('git not found on PATH');
	});
});
