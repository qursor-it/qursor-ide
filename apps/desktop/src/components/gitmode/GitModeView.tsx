import { useGitModeStore } from '@store/gitModeStore';

/** The full-window view of the selected Git tab (specs/05-features.md F35). */
export function GitModeView() {
	const activeDir = useGitModeStore((s) => s.activeDir);
	return (
		<div data-testid="git-mode-view" className="flex min-h-0 flex-1 items-center justify-center">
			Git — {activeDir ?? 'no tab open'}
		</div>
	);
}
