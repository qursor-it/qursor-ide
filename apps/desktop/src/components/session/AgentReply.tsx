import type { AgentId, TerminalStatus } from '@factorai/types';
import { Button, Textarea } from '@factorai/ui';
import { pressKey, readSessionTail, sendToSession } from '@components/terminal/Terminal';
import { agentName } from '@lib/agents';
import { type PermissionPrompt, parsePermissionPrompt } from '@lib/permissionPrompt';
import { CornerDownLeft, MessageCircleQuestion } from 'lucide-react';
import { useEffect, useState } from 'react';

/** Lines read off the bottom of the terminal to find a permission menu. */
const TAIL_LINES = 24;
/** The title that flips the status can land a frame before the menu is drawn. */
const READ_DELAY_MS = 150;

/**
 * Over the terminal while the agent waits for you (F39): what it asks, and —
 * when it is Claude's numbered permission menu — a button per option, each
 * pressing that number exactly as the keyboard would. Anything else is answered
 * in the terminal or the reply box; nothing is ever sent without a click.
 */
export function WaitingBanner({
	sessionId,
	status,
	agent,
	focusTerminal,
}: {
	sessionId: string;
	status: TerminalStatus | undefined;
	agent: AgentId | null;
	focusTerminal: () => void;
}) {
	const [prompt, setPrompt] = useState<PermissionPrompt | null>(null);
	useEffect(() => {
		setPrompt(null);
		if (status !== 'waiting_input') return;
		const timer = setTimeout(
			() => setPrompt(parsePermissionPrompt(readSessionTail(sessionId, TAIL_LINES))),
			READ_DELAY_MS,
		);
		return () => clearTimeout(timer);
	}, [sessionId, status]);

	if (status !== 'waiting_input') return null;
	const name = agentName(agent);
	return (
		<div
			data-testid="waiting-banner"
			role="status"
			className="mx-4 mt-3 flex items-start gap-3 rounded-xl border border-primary/30 bg-primary/10 px-4 py-3"
		>
			<MessageCircleQuestion className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden />
			<div className="min-w-0 flex-1">
				<div className="font-medium text-sm">
					{prompt?.question ?? `${name} is waiting for you`}
				</div>
				<div className="mt-2 flex flex-wrap gap-1.5">
					{prompt?.options.map((o, i) => (
						<Button
							key={o.key}
							size="sm"
							variant={i === 0 ? 'default' : 'outline'}
							data-testid={`waiting-option-${o.key}`}
							title={`Press ${o.key} in the terminal`}
							onClick={() => {
								pressKey(sessionId, o.key);
								focusTerminal();
							}}
						>
							<span className="max-w-72 truncate">{o.label}</span>
							<kbd className="rounded border border-current/30 px-1 font-sans text-xs opacity-80">
								{o.key}
							</kbd>
						</Button>
					))}
					<Button size="sm" variant="quiet" onClick={focusTerminal}>
						Answer in the terminal
					</Button>
				</div>
			</div>
		</div>
	);
}

/**
 * A reply box under the terminal (F39): Enter sends the text and an Enter to
 * the agent, Shift+Enter is a new line. The text goes out through the same
 * paste path a file drop uses, so the agent sees what it would have seen typed.
 */
export function ReplyBox({
	sessionId,
	live,
	agent,
}: {
	sessionId: string;
	live: boolean;
	agent: AgentId | null;
}) {
	const [text, setText] = useState('');
	const send = () => {
		if (!text.trim()) return;
		if (sendToSession(sessionId, text, true)) setText('');
	};
	return (
		<div className="flex items-end gap-2 border-border border-t bg-card px-3 py-2">
			<Textarea
				data-testid="reply-box"
				rows={1}
				value={text}
				disabled={!live}
				onChange={(e) => setText(e.target.value)}
				onKeyDown={(e) => {
					if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
						e.preventDefault();
						send();
					}
				}}
				placeholder={live ? `Reply to ${agentName(agent)}…` : 'The session is not running'}
				aria-label={`Reply to ${agentName(agent)}`}
				className="max-h-32 min-h-8 resize-none py-1.5 text-sm"
			/>
			<Button
				size="sm"
				disabled={!live || !text.trim()}
				data-testid="reply-send"
				onClick={send}
				aria-label="Send to the agent"
				title="Send to the agent (Enter) · new line with Shift+Enter"
			>
				<CornerDownLeft />
			</Button>
		</div>
	);
}
