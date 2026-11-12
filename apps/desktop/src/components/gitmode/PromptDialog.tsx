import {
	Button,
	Dialog,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	Input,
} from '@factorai/ui';
import { useState } from 'react';

interface PromptDialogProps {
	title: string;
	/** Null closes it. */
	initial: string | null;
	confirmLabel: string;
	onCancel: () => void;
	onSubmit: (value: string) => void;
}

/** One-line name prompt for a new or renamed branch. */
export function PromptDialog({
	title,
	initial,
	confirmLabel,
	onCancel,
	onSubmit,
}: PromptDialogProps) {
	return (
		<Dialog open={initial !== null} onOpenChange={(open) => !open && onCancel()}>
			<DialogContent>
				{initial !== null && (
					<PromptBody
						title={title}
						initial={initial}
						confirmLabel={confirmLabel}
						onCancel={onCancel}
						onSubmit={onSubmit}
					/>
				)}
			</DialogContent>
		</Dialog>
	);
}

function PromptBody({
	title,
	initial,
	confirmLabel,
	onCancel,
	onSubmit,
}: Omit<PromptDialogProps, 'initial'> & { initial: string }) {
	const [value, setValue] = useState(initial);
	const name = value.trim();
	return (
		<form
			onSubmit={(e) => {
				e.preventDefault();
				if (name) onSubmit(name);
			}}
		>
			<DialogHeader>
				<DialogTitle>{title}</DialogTitle>
			</DialogHeader>
			<Input
				autoFocus
				aria-label={title}
				value={value}
				onChange={(e) => setValue(e.target.value)}
				className="my-4"
			/>
			<DialogFooter>
				<Button type="button" variant="outline" onClick={onCancel}>
					Cancel
				</Button>
				<Button type="submit" disabled={!name}>
					{confirmLabel}
				</Button>
			</DialogFooter>
		</form>
	);
}
