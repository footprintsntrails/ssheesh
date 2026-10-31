// Saving a store to disk without slowing the app down: a write waits a
// moment so a burst of changes (dragging, typing in a search) saves once,
// and a change to something that isn't saved doesn't write at all. Anything
// pending is saved when the window closes.

import { createJSONStorage, type PersistStorage, type StorageValue } from "zustand/middleware";

const DELAY_MS = 300;

export function quietStorage<S>(getStorage: () => Storage = () => localStorage): PersistStorage<S> | undefined {
  // No storage at all (tests, some previews): nothing is saved.
  const inner = createJSONStorage<S>(getStorage);
  if (!inner) return undefined;
  let last: Record<string, unknown> | null = null;
  let pending: { name: string; value: StorageValue<S> } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    if (pending) void inner.setItem(pending.name, pending.value);
    pending = null;
  };
  if (typeof window !== "undefined") {
    window.addEventListener("pagehide", flush);
    window.addEventListener("beforeunload", flush);
  }
  return {
    getItem: (name) => inner.getItem(name),
    removeItem: (name) => inner.removeItem(name),
    setItem(name, value) {
      // The saved part is rebuilt on every change, but its pieces keep their
      // identity until they really change.
      const state = value.state as Record<string, unknown>;
      if (last && Object.keys(state).length === Object.keys(last).length && Object.keys(state).every((k) => state[k] === last![k])) return;
      last = state;
      pending = { name, value };
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(flush, DELAY_MS);
    },
  };
}
