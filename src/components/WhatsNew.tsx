// "What's new": shown once after an update, with the notes of every version
// you skipped. Also opens from Settings → About and the command palette.

import { useEffect, useRef, type ReactNode } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { releasesSince, RELEASES, type Release } from "../lib/changelog";
import { ONBOARDING_VERSION, useSettings } from "../lib/settings";
import { useUpdates } from "../lib/updates";
import { Modal } from "./Overlays";
import { Button, Logo } from "./ui";

export const useWhatsNew = create<{ releases: Release[] | null; show(r: Release[]): void; close(): void }>((set) => ({
  releases: null,
  show: (releases) => set({ releases }),
  close: () => set({ releases: null }),
}));

/** Opens the notes of the version you're running. */
export function showCurrentNotes() {
  const current = useUpdates.getState().current;
  const r = RELEASES.filter((x) => x.version === current);
  useWhatsNew.getState().show(r.length ? r : RELEASES.slice(0, 1));
}

/** **bold**, `code` and [links](…) inside a line. */
function inline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/g).map((p, i) => {
    if (p.startsWith("**") && p.endsWith("**")) return <strong key={i} className="font-semibold text-fg">{p.slice(2, -2)}</strong>;
    if (p.startsWith("`") && p.endsWith("`"))
      return (
        <code key={i} className="rounded bg-ink/[0.07] px-1 font-mono text-[11.5px]">
          {p.slice(1, -1)}
        </code>
      );
    const link = /^\[([^\]]+)\]\([^)]+\)$/.exec(p);
    return link ? link[1] : p;
  });
}

function Notes({ body }: { body: string }) {
  const blocks: ReactNode[] = [];
  let list: string[] = [];
  const flush = () => {
    if (!list.length) return;
    const items = list;
    blocks.push(
      <ul key={blocks.length} className="mt-1.5 flex list-disc flex-col gap-1.5 pl-4 marker:text-fg-3">
        {items.map((l, i) => (
          <li key={i}>{inline(l)}</li>
        ))}
      </ul>,
    );
    list = [];
  };
  for (const line of body.split("\n")) {
    if (line.startsWith("- ")) list.push(line.slice(2));
    else {
      flush();
      if (line.startsWith("### ")) blocks.push(<h4 key={blocks.length} className="mt-4 text-[12.5px] font-semibold text-fg">{line.slice(4)}</h4>);
      else if (line.trim()) blocks.push(<p key={blocks.length} className="mt-2">{inline(line)}</p>);
    }
  }
  flush();
  return <div className="text-[12.5px] leading-relaxed text-fg-2">{blocks}</div>;
}

export function WhatsNew() {
  const releases = useWhatsNew((s) => s.releases);
  const close = useWhatsNew((s) => s.close);
  const current = useUpdates((u) => u.current);
  const decided = useRef(false);

  // Once per launch, when the version is known: after an update, show what
  // changed since the version you last saw. A fresh install just remembers it.
  useEffect(() => {
    if (!current || decided.current) return;
    decided.current = true;
    const st = useSettings.getState();
    const seen = st.lastSeenVersion;
    if (seen === current) return;
    const demoQuiet = backend.isDemo && !new URLSearchParams(location.search).has("whatsnew");
    // No version on record: a new install (the welcome covers it), or an update
    // from before SSHeesh remembered versions.
    const updated = seen ? true : st.onboarded;
    st.update({ lastSeenVersion: current });
    // A new hello shows the changes itself; the notes stay in the command bar.
    const hello = st.onboardingVersion < ONBOARDING_VERSION;
    if (!updated || demoQuiet || hello) return;
    const r = releasesSince(seen || null, current);
    if (r.length) useWhatsNew.getState().show(seen ? r : r.slice(0, 1));
  }, [current]);

  const list = releases ?? [];
  // Focus the button without scrolling the notes away from their start.
  const gotIt = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (list.length) requestAnimationFrame(() => gotIt.current?.focus({ preventScroll: true }));
  }, [list.length]);
  return (
    <Modal open={list.length > 0} onClose={close} width={560}>
      <div className="p-6">
        <div className="flex items-center gap-3">
          <Logo size={44} winking />
          <div>
            <h2 className="text-[18px] font-semibold tracking-tight text-fg">What's new in SSHeesh {list[0]?.version}</h2>
            {list.length > 1 && <p className="text-[12px] text-fg-3">And everything since {list.at(-1)?.version}.</p>}
          </div>
        </div>
        <div className="mt-4 flex flex-col gap-6">
          {list.map((r) => (
            <section key={r.version}>
              {list.length > 1 && (
                <h3 className="text-[14px] font-semibold text-fg">
                  {r.version} <span className="font-normal text-fg-3">· {r.title}</span>
                </h3>
              )}
              <Notes body={r.body} />
            </section>
          ))}
        </div>
        <div className="mt-6 flex justify-end">
          <Button ref={gotIt} variant="primary" onClick={close}>
            Got it
          </Button>
        </div>
      </div>
    </Modal>
  );
}
