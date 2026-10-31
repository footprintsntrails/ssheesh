// Whether the tour is open, and at which step. Apart from the tour itself,
// so anything can open it without loading it.

import { create } from "zustand";

export const useTour = create<{ open: boolean; start: number; show(step?: number): void; close(): void }>((set) => ({
  open: false,
  start: 0,
  show: (step = 0) => set({ open: true, start: step }),
  close: () => set({ open: false }),
}));
