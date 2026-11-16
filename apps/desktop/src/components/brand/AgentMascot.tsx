import type { SVGProps } from 'react';

/**
 * Our agent mark (ADR-0100): a terminal with a `>_` prompt for a face, the
 * agent at its command line. Drawn on lucide's 24px grid with a 2px stroke in
 * `currentColor`, so it sits beside the other rail glyphs and takes their
 * colour and size.
 */
export function AgentMascot(props: SVGProps<SVGSVGElement>) {
	return (
		<svg
			xmlns="http://www.w3.org/2000/svg"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth={2}
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
			{...props}
		>
			<rect x="3" y="4" width="18" height="16" rx="4.5" />
			<path d="M7.5 10l2.5 2-2.5 2" />
			<path d="M12.5 14.5h4" />
		</svg>
	);
}
