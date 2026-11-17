import type { Project, SidebarRow } from '@factorai/types';
import { Button } from '@factorai/ui';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useCanvasStore } from '@store/canvasStore';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Shapes } from 'lucide-react';
import { useState } from 'react';
import { NewCanvasDialog } from './NewCanvasDialog';
import { useHeldCanvasPaths } from './useCanvasDoc';

/** Projects in the sidebar's order, groups flattened (F44). */
const flatProjects = (rows: SidebarRow[]): Project[] =>
	rows.flatMap((r) => (r.kind === 'project' ? [r.project] : r.children.map((c) => c.project)));

/** Each project with its canvases, and the door to a new one (F44). */
export function CanvasSidebar() {
	const sidebarQ = useQuery({ queryKey: queryKeys.sidebar(), queryFn: () => cmd.listSidebar() });
	const projects = flatProjects(sidebarQ.data ?? []);
	return (
		<nav
			data-testid="canvas-sidebar"
			aria-label="Canvases"
			className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-1.5 py-1"
		>
			{projects.map((p) => (
				<ProjectCanvases key={p.id} project={p} />
			))}
		</nav>
	);
}

function ProjectCanvases({ project }: { project: Project }) {
	const queryClient = useQueryClient();
	const selected = useCanvasStore((s) => s.selected);
	const select = useCanvasStore((s) => s.select);
	const [creating, setCreating] = useState(false);
	const unsaved = useHeldCanvasPaths();
	// A project with no folder on disk has nowhere to keep a canvas.
	const hasFolder = !!project.realPath && !project.missing;
	const canvasesQ = useQuery({
		queryKey: queryKeys.canvases(project.id),
		queryFn: () => cmd.listCanvases(project.id),
		enabled: hasFolder,
		refetchInterval: 15_000,
		refetchOnWindowFocus: true,
	});
	const create = useMutation({
		mutationFn: (title: string) => cmd.createCanvas(project.id, title),
		onSuccess: (entry) => {
			void queryClient.invalidateQueries({ queryKey: queryKeys.canvases(project.id) });
			select({ projectId: project.id, path: entry.path });
			setCreating(false);
		},
	});

	return (
		<section data-testid="canvas-project" data-project={project.id}>
			<div className="flex h-7 items-center gap-1.5 px-2 text-sm">
				<ProjectIcon name={project.displayName} size={18} />
				<span className="min-w-0 flex-1 truncate font-medium">{project.displayName}</span>
			</div>
			{(canvasesQ.data ?? []).map((c) => {
				const isSelected = selected?.path === c.path;
				return (
					<Button
						key={c.path}
						variant="ghost"
						data-testid="canvas-row"
						aria-current={isSelected}
						title={c.relPath}
						className={`h-7 w-full justify-start gap-1.5 px-2 pl-7 font-normal text-sm hover:bg-secondary/60 hover:text-foreground [&_svg]:size-3.5 ${
							isSelected ? 'bg-background text-foreground shadow-sm' : 'text-secondary-foreground'
						}`}
						onClick={() => select({ projectId: project.id, path: c.path })}
					>
						<Shapes />
						<span className="truncate">{c.title}</span>
						{unsaved.includes(c.path) && (
							<span
								data-testid="canvas-unsaved-dot"
								role="img"
								aria-label="Unsaved changes"
								title="Unsaved changes"
								className="size-1.5 shrink-0 rounded-full bg-amber-400"
							/>
						)}
					</Button>
				);
			})}
			{hasFolder && (
				<Button
					variant="ghost"
					data-testid="new-canvas"
					className="h-7 w-full justify-start gap-1.5 px-2 pl-7 font-normal text-muted-foreground text-sm hover:bg-secondary/60 hover:text-foreground [&_svg]:size-3.5"
					onClick={() => setCreating(true)}
				>
					<Plus />
					New canvas
				</Button>
			)}
			<NewCanvasDialog
				projectName={creating ? project.displayName : null}
				error={create.error ? formatError(create.error) : null}
				pending={create.isPending}
				onCancel={() => {
					setCreating(false);
					create.reset();
				}}
				onSubmit={(title) => create.mutate(title)}
			/>
		</section>
	);
}
