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

interface NewCanvasDialogProps {
	/** Null closes it. */
	projectName: string | null;
	error: string | null;
	pending: boolean;
	onCancel: () => void;
	onSubmit: (title: string) => void;
}

/** One-line title prompt for a new canvas (F44). */
export function NewCanvasDialog({
	projectName,
	error,
	pending,
	onCancel,
	onSubmit,
}: NewCanvasDialogProps) {
	return (
		<Dialog open={projectName !== null} onOpenChange={(open) => !open && onCancel()}>
			<DialogContent>
				{projectName !== null && (
					<Body
						projectName={projectName}
						error={error}
						pending={pending}
						onCancel={onCancel}
						onSubmit={onSubmit}
					/>
				)}
			</DialogContent>
		</Dialog>
	);
}

function Body({
	projectName,
	error,
	pending,
	onCancel,
	onSubmit,
}: Omit<NewCanvasDialogProps, 'projectName'> & { projectName: string }) {
	const [value, setValue] = useState('');
	const title = value.trim();
	return (
		<form
			onSubmit={(e) => {
				e.preventDefault();
				if (title && !pending) onSubmit(title);
			}}
		>
			<DialogHeader>
				<DialogTitle>New canvas in {projectName}</DialogTitle>
			</DialogHeader>
			<Input
				autoFocus
				aria-label="Canvas title"
				placeholder="Brainstorm MVP"
				value={value}
				onChange={(e) => setValue(e.target.value)}
				className="my-4"
			/>
			{error && (
				<p role="alert" className="-mt-2 mb-4 text-destructive text-sm">
					{error}
				</p>
			)}
			<DialogFooter>
				<Button type="button" variant="outline" onClick={onCancel}>
					Cancel
				</Button>
				<Button type="submit" disabled={!title || pending}>
					Create
				</Button>
			</DialogFooter>
		</form>
	);
}
