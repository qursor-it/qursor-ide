import { Button } from '@factorai/ui';
import { hunkPatch, splitHunks } from '@lib/hunks';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQuery } from '@tanstack/react-query';
import { useGitOp } from './useGitOp';

const LINE_CLASS: Record<string, string> = {
	'+': 'bg-emerald-500/10 text-emerald-300',
	'-': 'bg-destructive/10 text-destructive',
};
const SMALL = 'h-5 px-1.5 text-xs';

/** One file's patch with a button per hunk (F35 hunk staging). */
export function HunkDiff({ dir, path, staged }: { dir: string; path: string; staged: boolean }) {
	const { run, running } = useGitOp(dir);
	const patchQ = useQuery({
		queryKey: queryKeys.gitFilePatch(dir, path, staged),
		queryFn: () => cmd.gitFilePatch(dir, path, staged),
	});
	if (patchQ.isPending) return <div className="p-3 text-muted-foreground text-sm">Loading…</div>;
	const { header, hunks } = splitHunks(patchQ.data ?? '');
	if (hunks.length === 0)
		return <div className="p-3 text-muted-foreground text-sm">No textual changes.</div>;

	return (
		<div data-testid="hunk-diff" className="min-h-0 flex-1 overflow-auto font-mono text-xs">
			{hunks.map((hunk) => {
				const patch = hunkPatch(header, hunk);
				const [title = '', ...lines] = hunk.replace(/\n$/, '').split('\n');
				return (
					<section key={hunk} data-testid="hunk" className="border-b border-border">
						<div className="flex items-center gap-1.5 bg-card px-3 py-1 text-muted-foreground">
							<span className="min-w-0 flex-1 truncate">{title}</span>
							{staged ? (
								<Button
									variant="outline"
									size="sm"
									className={SMALL}
									disabled={running}
									onClick={() => run({ kind: 'applyPatch', patch, cached: true, reverse: true })}
								>
									Unstage hunk
								</Button>
							) : (
								<>
									<Button
										variant="outline"
										size="sm"
										className={SMALL}
										disabled={running}
										onClick={() => run({ kind: 'applyPatch', patch, cached: true, reverse: false })}
									>
										Stage hunk
									</Button>
									<Button
										variant="outline"
										size="sm"
										className={SMALL}
										disabled={running}
										onClick={() =>
											run(
												{ kind: 'applyPatch', patch, cached: false, reverse: true },
												{ confirm: 'Discard this hunk? The change is lost.' },
											)
										}
									>
										Discard hunk
									</Button>
								</>
							)}
						</div>
						{lines
							// Lines of a hunk are positional; there is no id to key on.
							.map((line, n) => ({ line, id: n }))
							.map(({ line, id }) => (
								<div key={id} className={`whitespace-pre px-3 ${LINE_CLASS[line[0] ?? ''] ?? ''}`}>
									{line || ' '}
								</div>
							))}
					</section>
				);
			})}
		</div>
	);
}
