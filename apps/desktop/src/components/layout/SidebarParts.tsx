import { Button, Input, cn } from '@factorai/ui';
import { ChevronRight, CircleDot, Plus, Search } from 'lucide-react';
import { type ReactNode, type Ref, useState } from 'react';

/**
 * The pieces every mode's sidebar body is built from (ADR-0117): the filter row
 * with the mode's primary action, section headings, and the "In progress"
 * section that opens each body.
 */

interface ToolbarProps {
	value: string;
	onChange: (value: string) => void;
	placeholder: string;
	inputRef?: Ref<HTMLInputElement>;
	/** The mode's one creating action: New session, New branch, New canvas, New server. */
	action: { label: string; onClick: () => void; disabled?: boolean; testId?: string };
}

/** The filter field and, beside it, the mode's primary `+`. */
export function SidebarToolbar({ value, onChange, placeholder, inputRef, action }: ToolbarProps) {
	return (
		<div className="flex shrink-0 items-center gap-1.5 px-3 py-2.5">
			<div className="relative min-w-0 flex-1">
				<Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2 size-3.5 text-muted-foreground" />
				<Input
					ref={inputRef}
					type="search"
					value={value}
					onChange={(e) => onChange(e.target.value)}
					placeholder={placeholder}
					aria-label={placeholder}
					className="pl-7"
				/>
			</div>
			<Button
				size="icon"
				aria-label={action.label}
				title={action.label}
				data-testid={action.testId}
				disabled={action.disabled}
				onClick={action.onClick}
				className="shrink-0 [&_svg]:size-4"
			>
				<Plus />
			</Button>
		</div>
	);
}

interface HeaderProps {
	title: ReactNode;
	/** IconButtons drawn at the right of the heading. */
	actions?: ReactNode;
	/** Given with `onToggle`, the heading collapses its section. */
	open?: boolean;
	onToggle?: () => void;
	className?: string;
	testId?: string;
}

/** A section heading: 12px uppercase label, actions at the right (DESIGN.md "Sidebar Sections"). */
export function SidebarSectionHeader({
	title,
	actions,
	open,
	onToggle,
	className,
	testId,
}: HeaderProps) {
	const label = 'font-medium text-muted-foreground text-xs uppercase tracking-wider';
	return (
		<div
			data-testid={testId}
			className={cn('group/sh flex h-7 shrink-0 items-center gap-1 px-3', className)}
		>
			{onToggle ? (
				<button
					type="button"
					aria-expanded={open}
					onClick={onToggle}
					className={cn('flex min-w-0 flex-1 items-center gap-1 text-left', label)}
				>
					<span className="truncate">{title}</span>
					<ChevronRight
						className={cn(
							'size-3 shrink-0 opacity-0 transition group-hover/sh:opacity-100',
							open && 'rotate-90',
						)}
					/>
				</button>
			) : (
				<span className={cn('flex min-w-0 flex-1 items-center gap-1', label)}>{title}</span>
			)}
			{actions}
		</div>
	);
}

/** How many rows "In progress" shows before it folds the rest behind a button. */
const LIVE_LIMIT = 5;

/**
 * What is happening now in this mode, first in the body (ADR-0117). Rendered
 * only while it has rows: an empty "In progress" would be a heading that says
 * nothing, on every quiet day. Drawn outside the mode's scroller, under the
 * toolbar, so it stays in view however long the library below it is.
 */
export function LiveSection({ rows }: { rows: ReactNode[] }) {
	const [all, setAll] = useState(false);
	if (rows.length === 0) return null;
	const shown = all ? rows : rows.slice(0, LIVE_LIMIT);
	return (
		<section data-testid="sidebar-live" aria-label="In progress" className="shrink-0 pb-1">
			<SidebarSectionHeader
				title={
					<>
						<CircleDot className="size-3 shrink-0 text-status-working" />
						In progress
					</>
				}
			/>
			<div className="flex flex-col gap-0.5 px-1.5">
				{shown}
				{rows.length > LIVE_LIMIT && (
					<Button
						variant="ghost"
						onClick={() => setAll(!all)}
						className={cn(LIVE_ROW, 'text-muted-foreground')}
					>
						{all ? 'Show fewer' : `${rows.length - LIVE_LIMIT} more…`}
					</Button>
				)}
			</div>
		</section>
	);
}

const LIVE_ROW =
	'h-7 w-full justify-start gap-2 px-2 font-normal text-sm hover:bg-secondary/60 hover:text-foreground [&_svg]:size-3.5 [&_svg]:shrink-0';

interface LiveRowProps {
	/** A status dot or a 14px icon. */
	mark: ReactNode;
	label: string;
	/** Where it is happening: the project, the server. Muted, after the label. */
	detail?: string;
	/** Right-aligned: a count, a pill. */
	meta?: ReactNode;
	onClick: () => void;
	testId?: string;
}

/** One row of "In progress": same height and padding in every mode. */
export function LiveRow({ mark, label, detail, meta, onClick, testId }: LiveRowProps) {
	return (
		<Button
			variant="ghost"
			data-testid={testId}
			title={detail ? `${label} · ${detail}` : label}
			onClick={onClick}
			className={cn(LIVE_ROW, 'text-secondary-foreground')}
		>
			<span className="flex size-3.5 shrink-0 items-center justify-center">{mark}</span>
			<span className="min-w-0 flex-1 truncate text-left">
				{label}
				{detail && <span className="text-muted-foreground"> · {detail}</span>}
			</span>
			{meta}
		</Button>
	);
}

/** The red count a live row carries when it is waiting on you. */
export function NeedsYouPill({ n }: { n: number }) {
	return (
		<span className="shrink-0 rounded-full bg-destructive px-1.5 font-medium text-[11px] text-white tabular-nums leading-4">
			{n}
		</span>
	);
}
