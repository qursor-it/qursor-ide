import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { AlertTriangle } from 'lucide-react';
import { answerConfirm, useGitUi } from './useGitOp';

/**
 * The bottom strip: the command the last write ran and git's own words,
 * unedited (specs/05-features.md F35). Renders nothing before the first write.
 */
export function OpLog({ dir }: { dir: string }) {
	const last = useGitUi((s) => s.logs[dir]);
	if (!last) return null;
	const text = last.stderr || last.stdout;
	return (
		<div
			data-testid="op-log"
			data-ok={last.ok}
			className="max-h-32 shrink-0 overflow-auto border-t border-border bg-card px-3 py-1.5 font-mono text-xs"
		>
			<div className={last.ok ? 'text-muted-foreground' : 'text-destructive'}>$ {last.command}</div>
			{text && <pre className="whitespace-pre-wrap text-secondary-foreground">{text}</pre>}
		</div>
	);
}

/** The question a destructive op asks every time, from `useGitOp`'s `confirm`. */
export function GitConfirmHost() {
	const confirm = useGitUi((s) => s.confirm);
	return (
		<Dialog open={confirm !== null} onOpenChange={(open) => !open && answerConfirm(false)}>
			<DialogContent>
				<DialogHeader>
					<DialogTitle className="flex items-center gap-2">
						<AlertTriangle className="size-5 text-destructive" />
						Are you sure?
					</DialogTitle>
					<DialogDescription>{confirm?.text ?? ''}</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="outline" onClick={() => answerConfirm(false)}>
						Cancel
					</Button>
					<Button variant="destructive" onClick={() => answerConfirm(true)}>
						Continue
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
