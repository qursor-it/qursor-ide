import { writeFileSync } from 'node:fs';

// ADR-0122. Report only the first root session of this process.
export const QursorSession = async () => {
	const path = process.env.QURSOR_OPENCODE_SESSION_SIGNAL;
	return {
		event: async ({ event }) => {
			const info = event.properties?.info;
			if (event.type !== 'session.created' || !path || !info?.id || info.parentID) return;
			try {
				writeFileSync(path, info.id, { flag: 'wx' });
			} catch {
				// A prior root session may already have claimed this terminal.
			}
		},
	};
};
