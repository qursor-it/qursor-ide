import { Input } from '@factorai/ui';
import { useState } from 'react';
import { useQuickAgent } from './useQuickAgent';

/** "Ask the agent" in the action bar: Enter sends the request; the proposal opens in the popover. */
export function AskAgentField({ dir }: { dir: string }) {
	const { enabled, busy, show } = useQuickAgent(dir);
	const [text, setText] = useState('');
	if (!enabled) return null;
	return (
		<form
			className="ml-auto w-72"
			onSubmit={(e) => {
				e.preventDefault();
				const request = text.trim();
				if (!request || busy) return;
				setText('');
				void show('Ask the agent', { kind: 'request', text: request }, { request });
			}}
		>
			<Input
				aria-label="Ask the agent"
				placeholder="✦ Ask the agent to do something with git"
				value={text}
				disabled={busy}
				onChange={(e) => setText(e.target.value)}
				className="h-7 text-xs"
			/>
		</form>
	);
}
