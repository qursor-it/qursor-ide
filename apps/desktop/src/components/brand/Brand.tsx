import { cn } from '@factorai/ui';

/**
 * The Qursor mark: a Q whose tail is an arrow cursor, in the brand's
 * blue-to-violet gradient. Served from `public/qursor-mark.svg` (copied from the
 * Qursor website), so it is full-colour only — it does not take `currentColor`.
 *
 * Decorative by default: the wordmark beside it already says "Qursor IDE".
 */
function BrandMark({ className }: { className?: string }) {
	return (
		<img
			src="/qursor-mark.svg"
			alt=""
			aria-hidden="true"
			draggable={false}
			className={cn('size-4.5 object-contain', className)}
		/>
	);
}

/**
 * The wordmark. One text node split by a span, so it still selects, copies and
 * reads as "Qursor IDE"; the last word takes the accent.
 */
export function BrandWordmark({ className }: { className?: string }) {
	return (
		<span className={cn('font-bold text-sm tracking-tight', className)}>
			Qursor <span className="text-primary">IDE</span>
		</span>
	);
}

/**
 * The app icon, for About (F29): the same gradient mark, larger. Kept as its own
 * export so callers need not care that the one-colour cut no longer exists.
 */
export function BrandIcon({ className }: { className?: string }) {
	return (
		<img
			src="/qursor-mark.svg"
			alt=""
			aria-hidden="true"
			draggable={false}
			className={cn('size-16 object-contain', className)}
		/>
	);
}

/** Mark plus wordmark, the lockup used in the header. */
export function Brand({ className }: { className?: string }) {
	return (
		<span className={cn('flex items-center gap-2', className)}>
			<BrandMark />
			<BrandWordmark />
		</span>
	);
}
