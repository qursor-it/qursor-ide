import type { GhStatus, PrDetail } from '@factorai/types';
import { Button } from '@factorai/ui';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft } from 'lucide-react';
import { FileChangeRow } from '@components/files/FileChangeRow';
import { useResetOnUnauthenticated } from '@hooks/useGhStatus';
import { DETAIL_POLL_MS, usePrFiles, usePrSelection } from '@hooks/usePrChanges';
import { ghErrorText } from '@lib/pullRequests';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { usePrTabStore } from '@store/prTabStore';

/**
 * The panel's PRs tab while a pull request's page is open (specs/05-features.md
 * F31): that pull request's changed files, in place of the list. Picking a file
 * shows its diff in the page; "Pull requests" goes back to the list without
 * leaving the tab.
 */
export function PrFilesPanel({
	projectId,
	root,
	status,
	number,
	onBack,
}: {
	projectId: string;
	root: string;
	status: GhStatus;
	number: number;
	onBack: () => void;
}) {
	const slug = status.repo?.slug ?? '';
	const tabTitle = usePrTabStore(
		(s) => s.tabs.find((t) => t.projectId === projectId && t.number === number)?.title,
	);
	// The page's own query: one cache entry, so this is a hit once the page loaded.
	const query = useQuery({
		queryKey: queryKeys.ghPr(slug, number),
		queryFn: () => cmd.ghPrDetail(root, number),
		staleTime: DETAIL_POLL_MS,
		retry: false,
	});
	useResetOnUnauthenticated(root, query.error);
	const title = query.data?.title ?? tabTitle;

	return (
		<div className="flex min-h-0 flex-1 flex-col" data-testid="pr-files-panel">
			<div className="flex shrink-0 flex-col gap-0.5 border-border border-b px-2 py-1.5">
				<Button
					variant="quiet"
					size="sm"
					className="-ml-1 h-6 gap-1 self-start px-1.5 font-normal text-muted-foreground text-xs"
					data-testid="pr-files-back"
					onClick={onBack}
				>
					<ChevronLeft />
					Pull requests
				</Button>
				<p className="line-clamp-2 px-0.5 text-xs" title={title} data-testid="pr-files-title">
					<span className="tabular-nums">#{number}</span>
					{title && ` ${title}`}
				</p>
			</div>
			<div className="min-h-0 flex-1 overflow-y-auto py-1">
				{query.isPending && <Note>Loading pull request…</Note>}
				{query.isError && <ErrorNote error={query.error} onRetry={() => void query.refetch()} />}
				{query.data && <PrFileList projectId={projectId} root={root} pr={query.data} />}
			</div>
		</div>
	);
}

/** The changed files of one pull request, the selected one marked. */
function PrFileList({ projectId, root, pr }: { projectId: string; root: string; pr: PrDetail }) {
	const query = usePrFiles(root, pr);
	const { selected, select } = usePrSelection(projectId, pr.number, query.data?.files);

	if (query.isPending) return <Note>Loading files…</Note>;
	if (query.isError) {
		return <ErrorNote error={query.error} onRetry={() => void query.refetch()} />;
	}
	const { files, total, truncated } = query.data;
	if (files.length === 0) return <Note>This pull request changes no files.</Note>;

	return (
		<>
			<ul data-testid="pr-files">
				{files.map((file) => (
					<FileChangeRow
						key={file.path}
						relPath={file.relPath}
						oldRelPath={file.oldRelPath}
						kind={file.kind}
						additions={file.additions}
						deletions={file.deletions}
						// Whether a file is binary is learned from its blob when the diff
						// opens (the same path a commit range takes), not from this row.
						isBinary={false}
						selected={selected?.relPath === file.relPath}
						onClick={() => select(file.relPath)}
					/>
				))}
			</ul>
			{(truncated || total > files.length) && (
				<p className="px-3 pb-2 text-muted-foreground/60 text-xs" data-testid="pr-files-truncated">
					… {total - files.length} more files on GitHub
				</p>
			)}
		</>
	);
}

function ErrorNote({ error, onRetry }: { error: unknown; onRetry: () => void }) {
	return (
		<div data-testid="pr-error" className="flex flex-col gap-2 px-3 py-2 text-xs">
			<p className="text-destructive">{ghErrorText(error)}</p>
			<Button variant="outline" size="sm" className="self-start" onClick={onRetry}>
				Retry
			</Button>
		</div>
	);
}

function Note({ children }: { children: string }) {
	return <p className="px-3 py-2 text-muted-foreground text-xs">{children}</p>;
}
