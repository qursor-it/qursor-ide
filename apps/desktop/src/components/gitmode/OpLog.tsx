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
