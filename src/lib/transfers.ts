// Uploads and downloads in progress, shown as cards in the corner: how far
// each got, how fast it goes, whether it's waiting for a dropped connection,
// and a button to stop it.

import { create } from "zustand";
import { backend, type TransferHooks } from "./backend";
import { newId } from "./layout";
import type { Cleaning, TransferProgress } from "./types";

export interface Transfer {
  id: string;
  kind: "upload" | "download" | "copy";
  /** File name, or "3 items". */
  label: string;
  done: number;
  /** 0 while the size isn't known. */
  total: number;
  /** Bytes per second, smoothed; 0 until measured. */
  rate: number;
  /** The connection dropped; it picks up again once the server answers. */
  waiting: boolean;
  cancelling: boolean;
  /** What came out of the files on the way, for uploads that clean them. */
  cleaning?: Cleaning;
}

export const useTransfers = create<{ items: Transfer[] }>(() => ({ items: [] }));

function patch(id: string, p: Partial<Transfer>) {
  useTransfers.setState((s) => ({ items: s.items.map((t) => (t.id === id ? { ...t, ...p } : t)) }));
}

/**
 * Shows a card for `run` while it goes. Resolves with its result, or rejects
 * with its error (code "cancelled" when you stopped it).
 */
export async function track<T>(kind: Transfer["kind"], label: string, run: (hooks: TransferHooks) => Promise<T>): Promise<T> {
  const id = newId("x");
  useTransfers.setState((s) => ({ items: [...s.items, { id, kind, label, done: 0, total: 0, rate: 0, waiting: false, cancelling: false }] }));
  let sample = { at: performance.now(), done: 0 };
  const onProgress = ({ done, total, waiting = false, cleaning }: TransferProgress) => {
    const t = useTransfers.getState().items.find((x) => x.id === id);
    if (!t) return;
    if (cleaning) patch(id, { cleaning });
    // No speed while waiting, and a fresh measure once it goes on.
    if (waiting || t.waiting) {
      sample = { at: performance.now(), done };
      patch(id, { done, total, waiting, rate: 0 });
      return;
    }
    // Speed over the last quarter second or more, smoothed so it doesn't jitter.
    const now = performance.now();
    const secs = (now - sample.at) / 1000;
    let rate = t.rate;
    if (secs >= 0.25) {
      const recent = Math.max(0, done - sample.done) / secs;
      rate = rate ? rate * 0.6 + recent * 0.4 : recent;
      sample = { at: now, done };
    }
    patch(id, { done, total, rate });
  };
  try {
    return await run({ id, onProgress });
  } finally {
    useTransfers.setState((s) => ({ items: s.items.filter((t) => t.id !== id) }));
  }
}

export function cancelTransfer(id: string) {
  patch(id, { cancelling: true });
  backend.cancelTransfer(id);
}
