import { Button } from '@factorai/ui';
import { useSettingsModal } from '@hooks/useSettingsModal';
import type { ErrorNext, ErrorNotice } from '@lib/intelligenceView';
import { Info, TriangleAlert } from 'lucide-react';
import type { ReactNode } from 'react';

/** The label of the control each kind of "what now" is. `settings` and `none`
 *  are not here: the first opens the switch, the second offers nothing. */
const RETRY_LABEL: Record<Exclude<ErrorNext, 'settings' | 'none'>, string> = {
	retry: 'Try again',
	'preview-again': 'Refresh the preview',
	'prepare-again': 'Prepare again',
	'check-again': 'Check again',
};

/**
 * A precise message with the one thing to do about it (specs/05-features.md F33
 * § "Error"; the original brief asks for "a precise explanation, retry or
 * opt-out").
 *
 * Three lines at most: what happened, what it means for the person's data, and
 * the backend's own sentence when it names a file or a count. The action is
 * chosen by `notice.next`, never by the caller, so the same code always offers
 * the same way out.
 */
export function Notice({
	notice,
	onRetry,
	testId = 'intelligence-notice',
	tone = 'error',
	children,
}: {
	notice: ErrorNotice;
	/** What the retry-shaped actions do. Absent hides them: a notice with a
	 *  button that cannot do anything is worse than one without. */
	onRetry?: () => void;
	testId?: string;
	/** `info` for a result that is degraded rather than broken: nothing went
	 *  wrong with the person's data, and red would say it had. */
	tone?: 'error' | 'info';
	/** Extra controls beside the action, e.g. a second way out. */
	children?: ReactNode;
}) {
	const settings = useSettingsModal();
	const retryLabel =
		notice.next === 'settings' || notice.next === 'none' ? null : RETRY_LABEL[notice.next];
	return (
		<div
			role={tone === 'error' ? 'alert' : 'status'}
			data-testid={testId}
			data-next={notice.next}
			className={`flex gap-2.5 rounded-md border px-3 py-2.5 ${
				tone === 'error'
					? 'border-destructive/40 bg-destructive/5'
					: 'border-border bg-secondary/30'
			}`}
		>
			{tone === 'error' ? (
				<TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden />
			) : (
				<Info className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
			)}
			<div className="min-w-0 flex-1 space-y-1">
				<p className="font-medium text-sm">{notice.title}</p>
				<p className="text-muted-foreground text-xs">{notice.hint}</p>
				{notice.detail && (
					<p className="break-words font-mono text-muted-foreground text-xs">{notice.detail}</p>
				)}
				{(retryLabel || notice.next === 'settings' || children) && (
					<div className="flex flex-wrap items-center gap-2 pt-1">
						{retryLabel && onRetry && (
							<Button
								variant="outline"
								size="sm"
								data-testid="intelligence-notice-retry"
								onClick={onRetry}
							>
								{retryLabel}
							</Button>
						)}
						{notice.next === 'settings' && (
							<Button
								variant="outline"
								size="sm"
								data-testid="intelligence-notice-settings"
								onClick={() => settings.open('intelligence')}
							>
								Open settings
							</Button>
						)}
						{children}
					</div>
				)}
			</div>
		</div>
	);
}
