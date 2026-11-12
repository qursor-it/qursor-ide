import type { GitOp, GitOpResult } from '@factorai/types';
import { isWithin } from '@lib/paths';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useTerminalStore } from '@store/terminalStore';
import { type QueryClient, useQueryClient } from '@tanstack/react-query';
import { useCallback, useState } from 'react';
import { create } from 'zustand';

/** What the OpLog shows: the command as run and git's own words. */
type OpLogEntry = GitOpResult;

interface GitUiState {
	/** The last op per directory, success or not. */
	logs: Record<string, OpLogEntry | undefined>;
	/** The destructive-op question being asked, if any. */
	confirm: { text: string; resolve: (ok: boolean) => void } | null;
}

export const useGitUi = create<GitUiState>()(() => ({ logs: {}, confirm: null }));

function ask(text: string): Promise<boolean> {
	return new Promise((resolve) => useGitUi.setState({ confirm: { text, resolve } }));
}

/** Answer the open confirm dialog. */
export function answerConfirm(ok: boolean): void {
	const pending = useGitUi.getState().confirm;
	useGitUi.setState({ confirm: null });
	pending?.resolve(ok);
}

/** One write at a time per repository (Review Focus 3). Module scope, so two
 *  components using the hook for one directory share it. */
const inFlight = new Set<string>();

function logFor(dir: string, entry: OpLogEntry): void {
	useGitUi.setState((s) => ({ logs: { ...s.logs, [dir]: entry } }));
}

/** The text of a rejected `invoke`: an `AppError` or an `Error`. */
function errorText(e: unknown): string {
	if (e && typeof e === 'object' && 'message' in e)
		return String((e as { message: unknown }).message);
	return String(e);
}

/**
 * Run one op: confirm if asked, refuse a second while one runs, log the
 * outcome, refresh what the view reads. Returns null when nothing ran.
 * Exported for its own test; components use `useGitOp`.
 */
export async function runGitOp(
	dir: string,
	op: GitOp,
	opts: { confirm?: string } | undefined,
	queryClient: Pick<QueryClient, 'invalidateQueries'>,
	setRunning: (running: boolean) => void = () => {},
): Promise<GitOpResult | null> {
	if (inFlight.has(dir)) return null;
	inFlight.add(dir);
	try {
		if (opts?.confirm && !(await ask(opts.confirm))) return null;
		setRunning(true);
		let result: GitOpResult;
		try {
			result = await cmd.gitOp(dir, op);
		} catch (e) {
			// A missing git, a spawn failure or a timeout rejects; it is logged
			// like a failed result so the user reads it in the same place.
			result = { command: `git ${op.kind}`, ok: false, stdout: '', stderr: errorText(e) };
		}
		logFor(dir, result);
		return result;
	} finally {
		inFlight.delete(dir);
		setRunning(false);
		for (const key of [
			queryKeys.gitStatus(dir),
			['git-graph', dir],
			queryKeys.gitRefs(dir),
			queryKeys.gitRepoState(dir),
			['git-file-patch', dir],
		]) {
			void queryClient.invalidateQueries({ queryKey: key });
		}
	}
}

/** Is a live agent of this project working in `dir`? (F35 "same checkout".) */
async function agentWorkingIn(dir: string, queryClient: QueryClient): Promise<boolean> {
	const projectId = useGitModeStore.getState().tabs.find((t) => t.dir === dir)?.projectId;
	if (!projectId) return false;
	const working = Object.entries(useTerminalStore.getState().bySession).filter(
		([, t]) => t.projectId === projectId && t.status === 'working',
	);
	if (working.length === 0) return false;
	const sessions = await queryClient
		.fetchQuery({
			queryKey: queryKeys.sessions(projectId),
			queryFn: () => cmd.listSessions(projectId),
			staleTime: 5_000,
		})
		.catch(() => []);
	return working.some(([id]) => {
		const cwd = sessions.find((s) => s.id === id)?.cwd;
		return cwd != null && isWithin(cwd, dir) && isWithin(dir, cwd);
	});
}

interface UseGitOp {
	run: (op: GitOp, opts?: { confirm?: string }) => Promise<GitOpResult | null>;
	running: boolean;
	last: GitOpResult | null;
}

/** Git writes for one directory (specs/05-features.md F35, ADR-0089). */
export function useGitOp(dir: string): UseGitOp {
	const queryClient = useQueryClient();
	const [running, setRunning] = useState(false);
	const last = useGitUi((s) => s.logs[dir] ?? null);
	const run = useCallback(
		async (op: GitOp, opts?: { confirm?: string }) => {
			// One question, not two: the agent warning leads the op's own.
			const warn =
				op.kind !== 'fetch' && (await agentWorkingIn(dir, queryClient))
					? 'An agent is working in this checkout. Continue?'
					: null;
			const confirm = warn ? [warn, opts?.confirm].filter(Boolean).join(' ') : opts?.confirm;
			return runGitOp(dir, op, { confirm }, queryClient, setRunning);
		},
		[dir, queryClient],
	);
	return { run, running, last };
}
