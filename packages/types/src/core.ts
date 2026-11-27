// The wire protocol of qursor-core, mirrored by hand from
// `qursor_core::protocol` (docs/superpowers/specs/2026-11-26-local-core-design.md §3).

import type {
	AgentRunEvent,
	AgentSessionStartEvent,
	AppError,
	ConfirmEvent,
	IdeOpenFileEvent,
	IdeStatusEvent,
	RoutineFireEvent,
	SessionAdoptedEvent,
	SessionWorktreeEvent,
	TerminalExitEvent,
	TerminalStatusEvent,
} from './index';

/** Binary frame opcodes: 1 output (core to client), 2 input, 3 resize. */
export type Op = 1 | 2 | 3;

export type ClientType = 'app' | 'renderer' | 'cli';

export type ClientMsg =
	| {
			type: 'hello';
			secret: string;
			clientId: string;
			clientType: ClientType;
			protocolVersion: number;
			appVersion: string;
			capabilities: string[];
	  }
	| { type: 'ping' }
	| { type: 'request'; requestId: number; method: string; params: unknown };

/** Terminal bytes never travel as an event: they are binary `Output` frames. */
export type CoreEvent =
	| { event: 'terminalStatus'; payload: TerminalStatusEvent }
	| { event: 'terminalExit'; payload: TerminalExitEvent }
	| { event: 'sessionAdopted'; payload: SessionAdoptedEvent }
	| { event: 'sessionWorktree'; payload: SessionWorktreeEvent }
	| { event: 'agentSessionStart'; payload: AgentSessionStartEvent }
	| { event: 'delegationsChanged'; payload: { projectId: string } }
	| { event: 'ideOpenFile'; payload: IdeOpenFileEvent }
	| { event: 'ideStatus'; payload: IdeStatusEvent }
	| { event: 'agentTerminalRun'; payload: AgentRunEvent }
	| { event: 'agentTerminalConfirm'; payload: ConfirmEvent }
	| { event: 'routinesChanged'; payload: { projectId: string } }
	| { event: 'routineFire'; payload: RoutineFireEvent }
	| { event: 'attention'; payload: AttentionNotice };

/** A session wants you and no client window is focused (F32); the app shows it. */
export interface AttentionNotice {
	sessionId: string | null;
	terminalId: string;
	title: string;
	body: string;
}

export type ServerMsg =
	| {
			type: 'server_info';
			hostId: string;
			version: string;
			protocolVersion: number;
			features: string[];
	  }
	| { type: 'pong' }
	| { type: 'response'; requestId: number; result: unknown }
	| { type: 'rpc_error'; requestId: number; error: AppError }
	| ({ type: 'event' } & CoreEvent);
