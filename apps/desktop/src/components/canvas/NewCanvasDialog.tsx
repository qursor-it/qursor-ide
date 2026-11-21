import {
	Button,
	Dialog,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	Input,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from '@factorai/ui';
import { useState } from 'react';

interface NewCanvasDialogProps {
	open: boolean;
	projects: { id: string; displayName: string }[];
	initialProjectId: string | null;
	error: string | null;
	pending: boolean;
	onCancel: () => void;
	onSubmit: (projectId: string, title: string) => void;
}

/** One-line title prompt for a new canvas (F44). */
export function NewCanvasDialog({
	open,
	projects,
	initialProjectId,
	error,
	pending,
	onCancel,
	onSubmit,
}: NewCanvasDialogProps) {
	return (
		<Dialog open={open} onOpenChange={(open) => !open && onCancel()}>
			<DialogContent>
				{open && projects.length > 0 && (
					<Body
						projects={projects}
						initialProjectId={initialProjectId}
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
	projects,
	initialProjectId,
	error,
	pending,
	onCancel,
	onSubmit,
}: Omit<NewCanvasDialogProps, 'open'>) {
	const [value, setValue] = useState('');
	const [picked, setPicked] = useState(initialProjectId ?? projects[0]?.id ?? '');
	const project = projects.find((p) => p.id === picked) ?? projects[0];
	const title = value.trim();
	return (
		<form
			onSubmit={(e) => {
				e.preventDefault();
				if (title && project && !pending) onSubmit(project.id, title);
			}}
		>
			<DialogHeader>
				<DialogTitle>
					{projects.length === 1 ? `New canvas in ${project?.displayName}` : 'New canvas'}
				</DialogTitle>
			</DialogHeader>
			{projects.length > 1 && (
				<Select value={project?.id} onValueChange={setPicked}>
					<SelectTrigger aria-label="Project" className="mt-4">
						<SelectValue />
					</SelectTrigger>
					<SelectContent>
						{projects.map((p) => (
							<SelectItem key={p.id} value={p.id}>
								{p.displayName}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			)}
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
