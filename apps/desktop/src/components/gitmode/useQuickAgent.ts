import type { GitAgentTask } from '@factorai/types';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { create } from 'zustand';

/** An answer waiting in the popover. `propose` answers carry git lines to run. */
interface AgentAnswer {
	title: string;
	text: string;
	propose: boolean;
	/** What `Open in a session` starts with. */
	request: string;
}

interface AgentUi {
	/** The label of the question in flight, per directory. */
	pending: Record<string, string | undefined>;
	answer: Record<string, AgentAnswer | undefined>;
	error: Record<string, string | undefined>;
}

export const useAgentUi = create<AgentUi>()(() => ({ pending: {}, answer: {}, error: {} }));

function patch(key: keyof AgentUi, dir: string, value: string | AgentAnswer | undefined): void {
	useAgentUi.setState((s) => ({ [key]: { ...s[key], [dir]: value } }));
}

function errorText(e: unknown): string {
	if (e && typeof e === 'object' && 'message' in e)
		return String((e as { message: unknown }).message);
	return String(e);
}

/** Split an agent's `# explanation` line from its `git ...` lines. */
export function parseProposal(text: string): { explanation: string; lines: string[] } {
	const all = text
		.split('\n')
		.map((l) => l.trim())
		.filter(Boolean);
	return {
		explanation: all
			.filter((l) => l.startsWith('# '))
			.map((l) => l.slice(2))
			.join(' '),
		lines: all.filter((l) => l.startsWith('git ')),
	};
}

interface QuickAgent {
	/** False for a Codex project: every quick `✦` button is hidden. */
	enabled: boolean;
	busy: boolean;
	/** The raw answer, or null on error, cancel or while another question runs. */
	ask: (label: string, task: GitAgentTask) => Promise<string | null>;
	/** Ask and show the answer in the popover; `propose` makes it a runnable proposal. */
	show: (label: string, task: GitAgentTask, propose?: { request: string }) => Promise<void>;
	cancel: () => void;
}

/** One-shot `claude -p` questions about the checkout `dir` (specs/05-features.md F35). */
export function useQuickAgent(dir: string): QuickAgent {
	const projectId = useGitModeStore((s) => s.tabs.find((t) => t.dir === dir)?.projectId);
	const busy = useAgentUi((s) => s.pending[dir] !== undefined);
	const defaultQ = useQuery({
		queryKey: queryKeys.setting('agentDefault'),
		queryFn: () => cmd.getSetting('agentDefault'),
	});
	const ask = useCallback(
		async (label: string, task: GitAgentTask) => {
			if (!projectId || useAgentUi.getState().pending[dir] !== undefined) return null;
			patch('pending', dir, label);
			patch('error', dir, undefined);
			try {
				return await cmd.gitAskAgent(projectId, dir, task);
			} catch (e) {
				const text = errorText(e);
				if (!text.endsWith('cancelled')) patch('error', dir, text);
				return null;
			} finally {
				patch('pending', dir, undefined);
			}
		},
		[projectId, dir],
	);
	const show = useCallback(
		async (label: string, task: GitAgentTask, propose?: { request: string }) => {
			const text = await ask(label, task);
			if (text) {
				patch('answer', dir, {
					title: label,
					text,
					propose: propose !== undefined,
					request: propose?.request ?? '',
				});
			}
		},
		[ask, dir],
	);
	const cancel = useCallback(() => void cmd.gitAskCancel(dir), [dir]);
	return { enabled: defaultQ.data !== 'codex', busy, ask, show, cancel };
}
