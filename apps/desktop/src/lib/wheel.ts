import type { WheelEvent } from 'react';

/** A vertical wheel over a horizontal tab strip scrolls it: by default it does
 *  nothing, which reads as "these tabs are stuck" (F16). */
export function wheelScrollsX(e: WheelEvent<HTMLElement>): void {
	if (e.deltaY !== 0) e.currentTarget.scrollLeft += e.deltaY;
}
