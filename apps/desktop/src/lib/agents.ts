import type { AgentId, SettingKey } from '@factorai/types';

/**
 * The agents factorai knows, in the order the Agents section shows them
 * (F30, ADR-0060). A mirror of `agents::registry()` in Rust: the id is the
 * `agent` column value, the name is what the UI calls it.
 */
export const AGENTS = [
	{
		id: 'claude',
		name: 'Claude Code',
		binary: 'claude',
		setting: 'claudeBinaryPath',
		indexesSessions: true,
	},
	{
		id: 'codex',
		name: 'Codex',
		binary: 'codex',
		setting: 'codexBinaryPath',
		indexesSessions: true,
	},
	{
		id: 'opencode',
		name: 'OpenCode',
		binary: 'opencode',
		setting: 'openCodeBinaryPath',
		indexesSessions: false,
	},
] as const satisfies readonly {
	id: AgentId;
	name: string;
	binary: string;
	setting: SettingKey;
	indexesSessions: boolean;
}[];

export type IndexedAgentId = Extract<(typeof AGENTS)[number], { indexesSessions: true }>['id'];

export function agentIndexesSessions(id: AgentId | null | undefined): boolean {
	return AGENTS.find((a) => a.id === id)?.indexesSessions ?? true;
}

export function canRestoreSessionTab(
	agent: AgentId | null | undefined,
	sessionId: string,
): boolean {
	return agent === 'opencode'
		? /^ses_[a-z0-9]{1,64}$/i.test(sessionId)
		: agentIndexesSessions(agent);
}

export function agentName(id: AgentId | string | null | undefined): string {
	return AGENTS.find((a) => a.id === id)?.name ?? String(id ?? '');
}

export function agentShortName(id: AgentId): string {
	return id === 'claude' ? 'Claude' : agentName(id);
}
