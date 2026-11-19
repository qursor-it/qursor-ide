import type { RemoteEntry, Server } from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
	IconButton,
	Input,
	toast,
} from '@factorai/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowUp, File, Folder, FolderPlus, MoreHorizontal, RefreshCw, Upload } from 'lucide-react';
import { useState } from 'react';
import { formatError } from '@lib/errors';
import { cmd, pickFile, pickSavePath } from '@lib/tauri';

const join = (dir: string, name: string) => `${dir.replace(/\/+$/, '')}/${name}`;
const parent = (dir: string) => dir.replace(/\/[^/]+\/?$/, '') || '/';

function size(n: number): string {
	if (n < 1024) return `${n} B`;
	if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
	if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
	return `${(n / 1024 ** 3).toFixed(1)} GB`;
}

/**
 * The server's files beside its SSH session (F46): browse, download, upload,
 * new folder, rename, delete. Over SFTP on the tab's own login; nothing is
 * installed on the server. Deleting asks first and never recurses.
 */
export function SftpPanel({ server }: { server: Server }) {
	const qc = useQueryClient();
	const [path, setPath] = useState(server.startDir ?? '');
	const [typed, setTyped] = useState<string | null>(null);
	const [deleting, setDeleting] = useState<RemoteEntry | null>(null);
	const key = ['sftp', server.id, path] as const;
	const listing = useQuery({ queryKey: key, queryFn: () => cmd.sftpList(server.id, path) });
	const dir = listing.data?.path ?? path;
	const refresh = () => qc.invalidateQueries({ queryKey: ['sftp', server.id] });
	const fail = (what: string) => (e: unknown) => toast.error(`${what}: ${formatError(e)}`);

	const download = async (e: RemoteEntry) => {
		const to = await pickSavePath(`Download ${e.name}`, e.name);
		if (!to) return;
		cmd
			.sftpDownload(server.id, join(dir, e.name), to)
			.then(() => toast.success(`Downloaded ${e.name}`))
			.catch(fail('Download failed'));
	};
	const upload = async () => {
		const from = await pickFile(`Upload to ${dir}`);
		if (!from) return;
		cmd.sftpUpload(server.id, from, dir).then(refresh).catch(fail('Upload failed'));
	};
	// No `window.prompt`: the webview's is not a native dialog here.
	const [naming, setNaming] = useState<{ entry: RemoteEntry | null; name: string } | null>(null);
	const mkdir = () => setNaming({ entry: null, name: '' });
	const rename = (e: RemoteEntry) => setNaming({ entry: e, name: e.name });
	const applyName = () => {
		const n = naming;
		setNaming(null);
		const name = n?.name.trim();
		if (!n || !name) return;
		if (!n.entry) {
			cmd.sftpMkdir(server.id, join(dir, name)).then(refresh).catch(fail('Could not create it'));
		} else if (name !== n.entry.name) {
			cmd
				.sftpRename(server.id, join(dir, n.entry.name), join(dir, name))
				.then(refresh)
				.catch(fail('Rename failed'));
		}
	};

	return (
		<aside data-testid="sftp-panel" className="flex w-80 shrink-0 flex-col border-border border-l">
			<div className="flex h-8 shrink-0 items-center gap-1 border-border border-b px-2 text-xs text-muted-foreground">
				<span className="flex-1 font-medium text-foreground">Files</span>
				<IconButton
					aria-label="Parent folder"
					title="Parent folder"
					onClick={() => setPath(parent(dir))}
				>
					<ArrowUp />
				</IconButton>
				<IconButton aria-label="Upload a file here" title="Upload a file here" onClick={upload}>
					<Upload />
				</IconButton>
				<IconButton aria-label="New folder" title="New folder" onClick={mkdir}>
					<FolderPlus />
				</IconButton>
				<IconButton aria-label="Refresh" title="Refresh" onClick={refresh}>
					<RefreshCw />
				</IconButton>
			</div>
			<form
				className="border-border border-b p-1.5"
				onSubmit={(e) => {
					e.preventDefault();
					if (typed !== null) setPath(typed);
					setTyped(null);
				}}
			>
				<Input
					aria-label="Remote folder"
					className="h-7 font-mono text-xs"
					value={typed ?? dir}
					onChange={(e) => setTyped(e.target.value)}
					onBlur={() => setTyped(null)}
				/>
			</form>
			<div className="min-h-0 flex-1 overflow-y-auto py-1 text-sm" data-testid="sftp-list">
				{listing.isError && (
					<p className="px-3 py-2 text-xs text-destructive">{formatError(listing.error)}</p>
				)}
				{listing.data?.entries.map((e) => (
					<div
						key={e.name}
						role="button"
						tabIndex={0}
						data-testid={`sftp-${e.name}`}
						onDoubleClick={() => {
							if (e.isDir) setPath(join(dir, e.name));
							else download(e);
						}}
						onKeyDown={(k) => {
							if (k.key !== 'Enter') return;
							if (e.isDir) setPath(join(dir, e.name));
							else download(e);
						}}
						className="group flex h-7 items-center gap-2 px-3 hover:bg-secondary/60"
					>
						{e.isDir ? (
							<Folder className="size-3.5 shrink-0 text-muted-foreground" />
						) : (
							<File className="size-3.5 shrink-0 text-muted-foreground" />
						)}
						<span className="min-w-0 flex-1 truncate">{e.name}</span>
						{!e.isDir && (
							<span className="text-xs text-muted-foreground tabular-nums">{size(e.size)}</span>
						)}
						<DropdownMenu>
							<DropdownMenuTrigger asChild>
								<IconButton
									aria-label={`${e.name} actions`}
									className="opacity-0 group-hover:opacity-100"
								>
									<MoreHorizontal />
								</IconButton>
							</DropdownMenuTrigger>
							<DropdownMenuContent align="end">
								{e.isDir ? (
									<DropdownMenuItem onSelect={() => setPath(join(dir, e.name))}>
										Open
									</DropdownMenuItem>
								) : (
									<DropdownMenuItem onSelect={() => download(e)}>Download…</DropdownMenuItem>
								)}
								<DropdownMenuItem onSelect={() => rename(e)}>Rename…</DropdownMenuItem>
								<DropdownMenuSeparator />
								<DropdownMenuItem className="text-destructive" onSelect={() => setDeleting(e)}>
									Delete…
								</DropdownMenuItem>
							</DropdownMenuContent>
						</DropdownMenu>
					</div>
				))}
				{listing.data?.truncated && (
					<p className="px-3 py-2 text-xs text-muted-foreground">
						Only the first 2000 entries are shown.
					</p>
				)}
			</div>
			<Dialog open={naming !== null} onOpenChange={(o) => !o && setNaming(null)}>
				<DialogContent>
					<DialogHeader>
						<DialogTitle>
							{naming?.entry ? `Rename ${naming.entry.name}` : 'New folder'}
						</DialogTitle>
						<DialogDescription>
							In {dir} on {server.name}.
						</DialogDescription>
					</DialogHeader>
					<form
						onSubmit={(e) => {
							e.preventDefault();
							applyName();
						}}
					>
						<Input
							autoFocus
							aria-label="Name"
							value={naming?.name ?? ''}
							onChange={(e) => setNaming((n) => (n ? { ...n, name: e.target.value } : n))}
						/>
					</form>
					<DialogFooter>
						<Button variant="outline" onClick={() => setNaming(null)}>
							Cancel
						</Button>
						<Button onClick={applyName}>{naming?.entry ? 'Rename' : 'Create'}</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
			<Dialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
				<DialogContent>
					<DialogHeader>
						<DialogTitle>Delete {deleting?.name}?</DialogTitle>
						<DialogDescription>
							On {server.name}, in {dir}.{' '}
							{deleting?.isDir ? 'Only an empty folder can be deleted here.' : ''} This cannot be
							undone.
						</DialogDescription>
					</DialogHeader>
					<DialogFooter>
						<Button variant="outline" onClick={() => setDeleting(null)}>
							Cancel
						</Button>
						<Button
							variant="destructive"
							onClick={() => {
								const e = deleting;
								setDeleting(null);
								if (e)
									cmd
										.sftpRemove(server.id, join(dir, e.name), e.isDir)
										.then(refresh)
										.catch(fail('Delete failed'));
							}}
						>
							Delete
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</aside>
	);
}
