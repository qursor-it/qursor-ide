import { FileChangeRow } from '@components/files/FileChangeRow';
import type { GitRef } from '@factorai/types';
import { Button, IconButton } from '@factorai/ui';
import { useFileViewer } from '@hooks/useFileViewer';
import { avatarFor } from '@lib/avatar';
import { formatAbsolute } from '@lib/format';
import { queryKeys } from '@lib/queryKeys';
import { cmd, copyText } from '@lib/tauri';
import { useQuery } from '@tanstack/react-query';
import { Copy } from 'lucide-react';
import type { ReactNode } from 'react';

/**
 * The selected commit, docked under Git mode's history the way desktop Git
 * clients lay it out (F35, ADR-0099): the author with a large avatar, then
 * refs, SHA and parents as labelled rows, the message, and the files.
 */
export function GitCommitPane({
	dir,
	sha,
	refs,
	onSelectSha,
}: {
	dir: string;
	sha: string;
	refs: readonly GitRef[];
	onSelectSha: (sha: string) => void;
}) {
	const { open } = useFileViewer();
	const q = useQuery({
		queryKey: queryKeys.gitCommit(dir, sha),
		queryFn: () => cmd.gitCommit(dir, sha),
		// A commit never changes.
		staleTime: Number.POSITIVE_INFINITY,
	});
	if (q.isPending) return <Note>Loading commit…</Note>;
	if (!q.data) return <Note>That commit is no longer in the repository.</Note>;
	const d = q.data;
	const avatar = avatarFor(d.authorName, d.authorEmail);

	return (
		<div data-testid="git-commit-pane" className="px-4 py-3 text-sm">
			<div className="grid grid-cols-[4.5rem_minmax(0,1fr)] items-baseline gap-x-3 gap-y-1">
				<span />
				<span className="text-muted-foreground text-xs uppercase tracking-wider">Author</span>
				<span className="flex justify-end">
					<span
						aria-hidden="true"
						style={{ background: avatar.colour, color: avatar.ink }}
						className="flex size-9 items-center justify-center rounded-md font-semibold text-sm"
					>
						{avatar.initials}
					</span>
				</span>
				<span className="flex flex-col">
					<span>
						<span className="font-semibold">{d.authorName}</span>{' '}
						<span className="text-muted-foreground text-xs">{d.authorEmail}</span>
					</span>
					<span className="text-muted-foreground text-xs">{formatAbsolute(d.authorTime)}</span>
				</span>
				{refs.length > 0 && (
					<Row label="Refs">
						<span className="flex flex-wrap gap-1">
							{refs.map((r) => (
								<span
									key={`${r.kind}:${r.name}`}
									className="rounded border border-border px-1.5 text-xs"
								>
									{r.name}
								</span>
							))}
						</span>
					</Row>
				)}
				<Row label="SHA">
					<span className="flex min-w-0 items-center gap-1 break-all font-mono text-xs">
						{d.sha}
						<IconButton aria-label="Copy SHA" title="Copy SHA" onClick={() => void copyText(d.sha)}>
							<Copy />
						</IconButton>
					</span>
				</Row>
				{d.parents.length > 0 && (
					<Row label={d.parents.length > 1 ? 'Parents' : 'Parent'}>
						<span className="flex gap-2">
							{d.parents.map((p) => (
								<Button
									key={p}
									variant="link"
									className="h-auto p-0 font-mono text-xs"
									onClick={() => onSelectSha(p)}
								>
									{p.slice(0, 7)}
								</Button>
							))}
						</span>
					</Row>
				)}
			</div>

			<div className="mt-3 border-border border-t pt-3 pl-[5.25rem]">
				<p className="font-semibold">
					{d.subject || <em className="text-muted-foreground">no message</em>}
				</p>
				{d.body && (
					<p className="mt-1 whitespace-pre-wrap font-mono text-muted-foreground text-xs">
						{d.body}
					</p>
				)}
			</div>

			<ul className="mt-3 border-border border-t pt-2">
				{d.files.map((file) => (
					<FileChangeRow
						key={file.path}
						relPath={file.relPath}
						oldRelPath={file.oldRelPath}
						kind={file.kind}
						additions={file.additions}
						deletions={file.deletions}
						isBinary={file.isBinary}
						onClick={() => open(file.path, { diff: `${d.diffParent ?? ''}..${d.sha}` })}
					/>
				))}
			</ul>
			{d.truncated && (
				<p className="px-3 text-muted-foreground text-xs">
					… {d.total - d.files.length} more files
				</p>
			)}
		</div>
	);
}

function Row({ label, children }: { label: string; children: ReactNode }) {
	return (
		<>
			<span className="text-right text-muted-foreground text-xs uppercase tracking-wider">
				{label}
			</span>
			{children}
		</>
	);
}

function Note({ children }: { children: string }) {
	return <p className="px-4 py-3 text-muted-foreground text-xs">{children}</p>;
}
