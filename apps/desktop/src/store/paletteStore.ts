import { create } from 'zustand';

/** Whether the command palette (F37) is open. Not persisted. */
export const usePaletteStore = create<{ open: boolean; setOpen: (open: boolean) => void }>()(
	(set) => ({ open: false, setOpen: (open) => set({ open }) }),
);
