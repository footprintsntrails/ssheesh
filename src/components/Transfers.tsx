// Cards for uploads and downloads in progress, stacked above the toasts.

import { ArrowDownToLine, ArrowUpFromLine, Copy, WifiOff, X } from "lucide-react";
import { bytes, cn, percent, rate } from "../lib/format";
import { cancelTransfer, useTransfers, type Transfer } from "../lib/transfers";

const VERB: Record<Transfer["kind"], string> = { upload: "Uploading", download: "Downloading", copy: "Copying" };
const ICON = { upload: ArrowUpFromLine, download: ArrowDownToLine, copy: Copy };

function TransferCard({ t }: { t: Transfer }) {
  const Icon = t.waiting ? WifiOff : ICON[t.kind];
  const known = t.total > 0;
  // A folder's size is an estimate, so the count can run a little past it.
  const done = known ? Math.min(t.done, t.total) : t.done;
  const fraction = known ? done / t.total : 0;
  return (
    <div role="status" className="glass pointer-events-auto animate-slide-up rounded-[13px] px-3.5 py-3">
      <div className="flex items-center gap-2.5">
        <Icon size={15} className={cn("shrink-0", t.waiting ? "text-warn" : "text-accent")} />
        <p className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-fg" title={t.label}>
          {t.label}
        </p>
        <button
          onClick={() => cancelTransfer(t.id)}
          disabled={t.cancelling}
          className="-mr-1 shrink-0 rounded p-0.5 text-fg-3 hover:text-fg disabled:opacity-40"
          aria-label={`Stop ${VERB[t.kind].toLowerCase()} ${t.label}`}
          title="Stop"
        >
          <X size={13} />
        </button>
      </div>
      <div className="mt-2 h-1 overflow-hidden rounded-full bg-ink/[0.08]">
        <div
          className={cn("h-full rounded-full transition-[width] duration-200", t.waiting ? "bg-warn" : "bg-accent", !known && "w-1/3 animate-sweep")}
          style={known ? { width: `${fraction * 100}%` } : undefined}
        />
      </div>
      <div className="mt-1.5 flex justify-between gap-2 text-[11.5px] tabular-nums text-fg-3">
        <span className="truncate">
          {t.cancelling
            ? "Stopping…"
            : t.waiting
              ? "Connection lost. Goes on once the server's back"
              : known
                ? `${bytes(done)} of ${bytes(t.total)} · ${percent(fraction)}`
                : `${VERB[t.kind]}… ${bytes(done)}`}
        </span>
        {!t.cancelling && !t.waiting && t.rate > 0 && <span className="shrink-0">{rate(t.rate)}</span>}
      </div>
    </div>
  );
}

export function TransferCards() {
  const items = useTransfers((s) => s.items);
  return items.map((t) => <TransferCard key={t.id} t={t} />);
}
