import { ProjectIcon } from '@components/layout/ProjectIcon';
import { PlanList, StatusPill } from '@components/session/HeaderChips';
import { sendToSession } from '@components/terminal/Terminal';
import { DiffView } from '@components/viewer/DiffView';
import type { GitChange } from '@factorai/types';
import { Button, Checkbox, Textarea } from '@factorai/ui';
import { useGitBranch } from '@hooks/useGitBranch';
import type { DiffMode } from '@hooks/useFileViewer';
import { lastAssistantText } from '@lib/agentSummary';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { createRoute, useNavigate } from '@tanstack/react-router';
import { ArrowLeft, Check } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';
import { rootRoute } from './__root';

/** The pair a change's diff compares, as the Changes tab picks it (F13). */
const diffOf = (c: GitChange): DiffMode =>
	c.group === 'staged' ? 'staged' : c.group === 'unstaged' ? 'unstaged' : 'head';
const keyOf = (c: GitChange) => `${c.group}:${c.path}`;

/** Events read off the end of the transcript for the agent's last words. */
const TAIL_EVENTS = 40;

function SideSection({ title, children }: { title: string; children: ReactNode }) {
	return (
		<section className="border-border border-b px-4 py-3 last:border-b-0">
			<h3 className="mb-2 font-medium text-muted-foreground text-xs uppercase tracking-wider">
				{title}
			</h3>
			{children}
		</section>
	);
}

/**
 * Review a session's changes on one screen (F43): the files on the left, each
 * markable as reviewed, the diff of the one picked in the middle, and on the
 * right what the agent said, its plan and a note to send back. Committing goes
 * to Git mode, where it always has; nothing here writes to git.
 */
function ReviewMode() {
	const { projectId, sessionId } = reviewRoute.useParams();
	const navigate = useNavigate();
	const live = useTerminalStore((s) => s.bySession[sessionId]);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const project = projectsQ.data?.find((p) => p.id === projectId);
	const root = project?.realPath ?? null;
	const branch = useGitBranch(root);
	const sessionsQ = useQuery({
		queryKey: queryKeys.sessions(projectId),
		queryFn: () => cmd.listSessions(projectId),
	});
	const session = sessionsQ.data?.find((s) => s.id === sessionId);
	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(root ?? ''),
		queryFn: () => cmd.gitStatus(root ?? ''),
		enabled: Boolean(root),
		refetchOnMount: 'always',
	});
	const tailQ = useQuery({
		queryKey: ['session-tail', sessionId, TAIL_EVENTS],
		queryFn: () => cmd.getSessionTail(sessionId, TAIL_EVENTS),
	});
	const planQ = useQuery({
		queryKey: queryKeys.sessionPlan(sessionId, live?.status ?? 'stopped'),
		queryFn: () => cmd.sessionPlan(sessionId),
	});

	const changes = statusQ.data?.repoRoot ? statusQ.data.changes : [];
	const [selected, setSelected] = useState<string | null>(null);
	const [reviewed, setReviewed] = useState<ReadonlySet<string>>(() => new Set());
	const [note, setNote] = useState('');
	const current = changes.find((c) => keyOf(c) === selected) ?? changes[0];

	const back = () =>
		void navigate({
			to: '/projects/$projectId/sessions/$sessionId',
			params: { projectId, sessionId },
		});
	useEffect(() => {
		const onKey = (e: KeyboardEvent) => e.key === 'Escape' && !e.defaultPrevented && back();
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	const add = changes.reduce((n, c) => n + (c.additions ?? 0), 0);
	const del = changes.reduce((n, c) => n + (c.deletions ?? 0), 0);
	const said = tailQ.data ? lastAssistantText(tailQ.data.events) : null;
	const plan = planQ.data ?? [];
	const toggle = (key: string) =>
		setReviewed((s) => {
			const next = new Set(s);
			if (!next.delete(key)) next.add(key);
			return next;
		});
	const sendNote = () => {
		const text = current ? `About ${current.relPath}: ${note.trim()}` : note.trim();
		if (note.trim() && sendToSession(sessionId, text, true)) {
			setNote('');
			back();
		}
	};
	const commit = () => {
		if (!root) return;
		const git = useGitModeStore.getState();
		git.openTab(projectId, root);
		git.setMode('git');
	};

	return (
		<main data-testid="review-mode" className="flex h-full flex-col">
			<header className="flex flex-col gap-1.5 border-border border-b bg-card px-4 py-2.5">
				<div className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
					<ProjectIcon name={project?.displayName ?? projectId} size={14} />
					<span className="truncate">{project?.displayName}</span>
					<span>/</span>
					<span className="truncate">{session?.title || 'Session'}</span>
					{branch && <span className="font-mono">· {branch}</span>}
				</div>
				<div className="flex min-w-0 items-center gap-3">
					<Button variant="quiet" size="sm" className="-ml-2 gap-1" onClick={back}>
						<ArrowLeft /> Back to session
					</Button>
					<h1 className="font-medium text-sm">Review changes</h1>
					<span className="rounded-full bg-primary/15 px-2 py-0.5 text-primary text-xs">
						{changes.length} {changes.length === 1 ? 'file' : 'files'} ·{' '}
						<span className="text-git-added">+{add}</span>{' '}
						<span className="text-git-deleted">−{del}</span>
					</span>
					<span className="flex-1" />
					{live && <StatusPill status={live.status} />}
					<Button size="sm" data-testid="review-commit" onClick={commit} disabled={!root}>
						Commit…
					</Button>
				</div>
			</header>

			{changes.length === 0 ? (
				<p className="p-6 text-muted-foreground text-sm">Nothing changed in this checkout.</p>
			) : (
				<div className="grid min-h-0 flex-1 grid-cols-[16rem_minmax(0,1fr)_20rem]">
					<nav
						aria-label="Changed files"
						className="min-h-0 overflow-y-auto border-border border-r p-2"
					>
						<div className="px-2 pt-1 pb-2 font-medium text-muted-foreground text-xs uppercase tracking-wider">
							Files{' '}
							<span data-testid="review-progress" className="normal-case tracking-normal">
								{reviewed.size} / {changes.length} reviewed
							</span>
						</div>
						<ul className="space-y-0.5">
							{changes.map((c) => {
								const key = keyOf(c);
								return (
									<li
										key={key}
										className={`flex items-center gap-2 rounded-md px-2 py-1 ${
											current && keyOf(current) === key ? 'bg-secondary' : 'hover:bg-secondary/60'
										}`}
									>
										<Checkbox
											aria-label={`Reviewed ${c.relPath}`}
											checked={reviewed.has(key)}
											onCheckedChange={() => toggle(key)}
										/>
										<Button
											variant="quiet"
											data-testid="review-file"
											onClick={() => setSelected(key)}
											className="h-auto min-w-0 flex-1 justify-start p-0 font-normal text-foreground text-sm hover:text-foreground"
										>
											<span className="truncate">{c.relPath}</span>
										</Button>
										{reviewed.has(key) && (
											<Check className="size-3.5 shrink-0 text-status-working" aria-hidden />
										)}
									</li>
								);
							})}
						</ul>
					</nav>

					<div className="flex min-h-0 min-w-0 flex-col">
						{current && (
							<DiffView key={keyOf(current)} path={current.path} mode={diffOf(current)} />
						)}
					</div>

					<aside className="min-h-0 overflow-y-auto border-border border-l">
						<SideSection title="What the agent said">
							<p
								data-testid="review-agent-said"
								className="whitespace-pre-wrap text-secondary-foreground text-sm"
							>
								{said ?? 'Nothing yet.'}
							</p>
						</SideSection>
						{plan.length > 0 && (
							<SideSection title="Plan">
								<PlanList plan={plan} />
							</SideSection>
						)}
						<SideSection title="Ask the agent to revise">
							<Textarea
								data-testid="review-note"
								rows={3}
								value={note}
								disabled={!live}
								onChange={(e) => setNote(e.target.value)}
								placeholder={
									live
										? current
											? `About ${current.relPath}…`
											: 'What should change…'
										: 'The session is not running'
								}
								className="text-sm"
							/>
							<Button
								size="sm"
								className="mt-2 w-full"
								data-testid="review-send"
								disabled={!live || !note.trim()}
								onClick={sendNote}
							>
								Send to the agent
							</Button>
						</SideSection>
					</aside>
				</div>
			)}
		</main>
	);
}

export const reviewRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/projects/$projectId/sessions/$sessionId/review',
	component: ReviewMode,
});
