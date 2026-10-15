import type { TerminalStatus } from '@factorai/types';

/**
 * F32's transition table: a session wants you when it goes from `working` to
 * `waiting_input` or `stopped`. Rust's `services::attention::wants_you` is the
 * same table, and each side tests it.
 */
export function wantsYou(prev: TerminalStatus | undefined, next: TerminalStatus): boolean {
	return prev === 'working' && (next === 'waiting_input' || next === 'stopped');
}

/** The `notifications.desktop` row as a switch: anything but `off` is on (F32). */
export function desktopNotificationsOf(value: string | null | undefined): boolean {
	return value !== 'off';
}

/** The row to write: on is the absence of one, like the update channel's stable. */
export function desktopNotificationsSetting(on: boolean): string | null {
	return on ? null : 'off';
}
