import type { GhStatus } from '@factorai/types';
import { Button } from '@factorai/ui';
import { useQueryClient } from '@tanstack/react-query';
import { Check, Copy } from 'lucide-react';
import { useState, type ReactNode } from 'react';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { PrDetailView } from '@components/prs/PrDetail';
import { PrList } from '@components/prs/PrList';
import { useActiveCheckout } from '@hooks/useActiveCheckout';
import { refreshGhStatus, useGhStatus } from '@hooks/useGhStatus';
import { isMacOS, isWindows } from '@lib/platform';
import { ghInstallCommand, ghStatusText } from '@lib/pullRequests';
import { copyText } from '@lib/tauri';
import { usePrPanelStore } from '@store/prPanelStore';

/**
 * The panel's PRs tab (specs/05-features.md F31).
 *
 * Gates on `gh_status` first: pull requests are read through the user's own
 * `gh`, so everything short of `ready` is an empty state that says how to get
 * there. The tab sits outside the panel's shared scroll wrapper, like the
 * graph — the list and the detail each own their scrolling — so its empty
 * states repeat the wrapper's 4px (`Empty`).
 */
export function PrsView() {
	// **The checkout, not the project folder** (F21), like Changes and the tree:
	// the pull request the agent's worktree belongs to is the one that matters,
	// and `gh_status` reads `origin` from whatever repository the path is in.
	const { projectId, root, isLoading } = useActiveCheckout();
	const status = useGhStatus(root);

	if (!projectId) return <Empty>Select a project to see its pull requests.</Empty>;
	if (isLoading || (root && status.isPending)) return <Empty>Loading…</Empty>;
	if (!root) return <Empty>Project folder not found on disk.</Empty>;
	if (!status.data) return <Empty>Could not check the GitHub CLI.</Empty>;

	const gh = status.data;
	if (gh.state !== 'ready' || !gh.repo) {
		return <Gate status={gh} root={root} />;
	}
	// Keyed on the repository, so the list's state cannot carry from one
	// repository's pull requests to another's when the checkout moves.
	return <Ready key={gh.repo.slug} projectId={projectId} root={root} status={gh} />;
}

function Ready({ projectId, root, status }: { projectId: string; root: string; status: GhStatus }) {
	const selected = usePrPanelStore((s) => s.selected[projectId] ?? null);
	const select = usePrPanelStore((s) => s.select);

	if (selected !== null) {
		return (
			<PrDetailView
				projectId={projectId}
				root={root}
				status={status}
				number={selected}
				onBack={() => select(projectId, null)}
			/>
		);
	}
	return <PrList root={root} status={status} onOpen={(n) => select(projectId, n)} />;
}

/** The empty state for a status that is not `ready`. */
function Gate({ status, root }: { status: GhStatus; root: string }) {
	const queryClient = useQueryClient();
	const [retrying, setRetrying] = useState(false);
	const retry = () => {
		setRetrying(true);
		void refreshGhStatus(queryClient, root)
			.catch(() => undefined)
			.finally(() => setRetrying(false));
	};
	const retryButton = (
		<Button
			variant="outline"
			size="sm"
			className="self-start"
			disabled={retrying}
			onClick={retry}
			data-testid="pr-retry"
		>
			Retry
		</Button>
	);

	if (status.state === 'notInstalled') {
		const platform = isWindows() ? 'windows' : isMacOS() ? 'mac' : 'other';
		const command = ghInstallCommand(platform);
		return (
			<Shell testId="pr-gate-not-installed">
				<p className="font-medium text-foreground text-sm">Install GitHub CLI</p>
				<p>Qursor IDE reads pull requests through the GitHub CLI (gh).</p>
				{command ? <CommandLine command={command} /> : <p>See cli.github.com for your system.</p>}
				<p>Then restart Qursor IDE.</p>
			</Shell>
		);
	}
	if (status.state === 'notAuthenticated') {
		return (
			<Shell testId="pr-gate-not-authenticated">
				<p className="font-medium text-foreground text-sm">Sign in to GitHub</p>
				<p>
					Run <code className="font-mono">gh auth login</code> in a terminal, then retry.
				</p>
				<CommandLine command="gh auth login" />
				{retryButton}
			</Shell>
		);
	}
	return (
		<Shell testId={`pr-gate-${status.state}`}>
			<p>{ghStatusText(status)}</p>
			{(status.state === 'unreachable' || status.state === 'tooOld') && retryButton}
		</Shell>
	);
}

function Shell({ children, testId }: { children: ReactNode; testId: string }) {
	return (
		<div
			data-testid={testId}
			className="flex flex-col gap-2 px-3 py-3 text-muted-foreground text-xs"
		>
			{children}
		</div>
	);
}

/** A command to run, with a copy button. The copy can be refused by the platform
 *  (see `copyText`), so a failure is said rather than ticked. */
function CommandLine({ command }: { command: string }) {
	const [copied, setCopied] = useState<'yes' | 'failed' | null>(null);
	const label = copied === 'failed' ? 'Copy failed' : copied === 'yes' ? 'Copied' : 'Copy command';
	return (
		<div className="flex items-center gap-1 rounded border border-border bg-background py-1 pr-1 pl-2">
			<code data-testid="pr-command" className="min-w-0 flex-1 truncate font-mono text-foreground">
				{command}
			</code>
			<Button
				variant="quiet"
				size="sm"
				className="h-6 shrink-0 gap-1 px-1.5 font-normal text-xs"
				aria-label={label}
				title={label}
				onClick={async () => {
					try {
						await copyText(command);
						setCopied('yes');
					} catch {
						setCopied('failed');
					}
					window.setTimeout(() => setCopied(null), 1200);
				}}
			>
				{copied === 'yes' ? <Check /> : <Copy />}
				{label === 'Copy command' ? 'Copy' : label}
			</Button>
		</div>
	);
}

/** `PanelEmpty` with the 4px of the scroll wrapper this tab sits outside. */
function Empty({ children }: { children: string }) {
	return (
		<div className="py-1">
			<PanelEmpty>{children}</PanelEmpty>
		</div>
	);
}
