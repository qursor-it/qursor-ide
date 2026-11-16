import type { SVGProps } from 'react';

/**
 * Our agent mascot (ADR-0100): a small robot head with an antenna spark and
 * two eyes, drawn on lucide's 24px grid with a 2px stroke in `currentColor`,
 * so it sits beside the other rail glyphs and takes their colour and size.
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
			{/* The antenna and its spark: the agent thinking. */}
			<path d="M12 7V4.5" />
			<path
				d="M12 1.6l.55 1.05 1.05.55-1.05.55L12 4.8l-.55-1.05-1.05-.55 1.05-.55z"
				fill="currentColor"
				strokeWidth={1}
			/>
			{/* The head. */}
			<rect x="4" y="7" width="16" height="13" rx="4.5" />
			{/* Ears. */}
			<path d="M2 12.5v3" />
			<path d="M22 12.5v3" />
			{/* Eyes and a small smile. */}
			<circle cx="9" cy="12.6" r="1.1" fill="currentColor" stroke="none" />
			<circle cx="15" cy="12.6" r="1.1" fill="currentColor" stroke="none" />
			<path d="M9.5 16.2c1.4 1 3.6 1 5 0" />
		</svg>
	);
}
