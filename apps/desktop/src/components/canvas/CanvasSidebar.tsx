import type { CanvasEntry, Project, SidebarRow } from '@factorai/types';
import { Button, IconButton } from '@factorai/ui';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useCanvasStore } from '@store/canvasStore';
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Shapes } from 'lucide-react';
import { useState } from 'react';
import { NewCanvasDialog } from './NewCanvasDialog';
import { useHeldCanvasPaths } from './useCanvasDoc';

/** Projects in the sidebar's order, groups flattened (F44). */
const flatProjects = (rows: SidebarRow[]): Project[] =>
	rows.flatMap((r) => (r.kind === 'project' ? [r.project] : r.children.map((c) => c.project)));

/** Each project that has canvases, and the door to a new one (F44). */
export function CanvasSidebar() {
	const queryClient = useQueryClient();
	const selected = useCanvasStore((s) => s.selected);
	const select = useCanvasStore((s) => s.select);
	const sidebarQ = useQuery({ queryKey: queryKeys.sidebar(), queryFn: () => cmd.listSidebar() });
	// A project with no folder on disk has nowhere to keep a canvas.
	const eligible = flatProjects(sidebarQ.data ?? []).filter((p) => !!p.realPath && !p.missing);
	const canvasQs = useQueries({
		queries: eligible.map((p) => ({
			queryKey: queryKeys.canvases(p.id),
			queryFn: () => cmd.listCanvases(p.id),
			refetchInterval: 15_000,
			refetchOnWindowFocus: true,
		})),
	});
	const [creatingIn, setCreatingIn] = useState<Project[] | null>(null);
	const create = useMutation({
		mutationFn: (v: { projectId: string; title: string }) => cmd.createCanvas(v.projectId, v.title),
		onSuccess: (entry, v) => {
			void queryClient.invalidateQueries({ queryKey: queryKeys.canvases(v.projectId) });
			select({ projectId: v.projectId, path: entry.path });
			setCreatingIn(null);
		},
	});
	const withCanvases = eligible.flatMap((p, i) => {
		const canvases = canvasQs[i]?.data ?? [];
		return canvases.length ? [{ project: p, canvases }] : [];
	});
	const anyLoading = canvasQs.some((q) => q.data === undefined);
	return (
		<nav
			data-testid="canvas-sidebar"
			aria-label="Canvases"
			className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-1.5 py-1"
		>
			<div className="flex h-7 items-center justify-end">
				<IconButton
					aria-label="New canvas"
					title="New canvas"
					data-testid="new-canvas-top"
					disabled={!eligible.length}
					onClick={() => setCreatingIn(eligible)}
				>
					<Plus />
				</IconButton>
			</div>
			{!withCanvases.length && !anyLoading && (
				<p className="px-2 text-muted-foreground text-sm">
					No canvases yet. Create one in any project.
				</p>
			)}
			{withCanvases.map(({ project, canvases }) => (
				<ProjectCanvases
					key={project.id}
					project={project}
					canvases={canvases}
					onNew={() => setCreatingIn([project])}
				/>
			))}
			<NewCanvasDialog
				open={creatingIn !== null}
				projects={creatingIn ?? []}
				initialProjectId={
					creatingIn?.find((p) => p.id === selected?.projectId)?.id ?? creatingIn?.[0]?.id ?? null
				}
				error={create.error ? formatError(create.error) : null}
				pending={create.isPending}
				onCancel={() => {
					setCreatingIn(null);
					create.reset();
				}}
				onSubmit={(projectId, title) => create.mutate({ projectId, title })}
			/>
		</nav>
	);
}

function ProjectCanvases({
	project,
	canvases,
	onNew,
}: {
	project: Project;
	canvases: CanvasEntry[];
	onNew: () => void;
}) {
	const selected = useCanvasStore((s) => s.selected);
	const select = useCanvasStore((s) => s.select);
	const unsaved = useHeldCanvasPaths();

	return (
		<section data-testid="canvas-project" data-project={project.id}>
			<div className="flex h-7 items-center gap-1.5 px-2 text-sm">
				<ProjectIcon name={project.displayName} size={18} />
				<span className="min-w-0 flex-1 truncate font-medium">{project.displayName}</span>
			</div>
			{canvases.map((c) => {
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
			<Button
				variant="ghost"
				data-testid="new-canvas"
				className="h-7 w-full justify-start gap-1.5 px-2 pl-7 font-normal text-muted-foreground text-sm hover:bg-secondary/60 hover:text-foreground [&_svg]:size-3.5"
				onClick={onNew}
			>
				<Plus />
				New canvas
			</Button>
		</section>
	);
}
