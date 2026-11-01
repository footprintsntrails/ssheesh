// Icon and color for a server: "Automatic" (what SSHeesh guessed it's for),
// a grid of icons, a row of colors, or a picture of your own.

import { Check, ImageIcon, ImagePlus, Plus, RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../lib/format";
import { HOST_COLORS, HOST_ICONS, hostImageFrom, isHostImage, lookOf, useLabels, type HostLook } from "../lib/labels";
import { detectRole } from "../lib/roles";
import { useSettings } from "../lib/settings";
import { ColorButton, RAINBOW } from "./ColorPicker";
import { Segmented } from "./controls";
import { IconButton } from "./ui";

/** Picture files a server's icon can be made from. */
const IMAGE_FILES = ".png,.jpg,.jpeg,.svg,.webp,.ico,image/png,image/jpeg,image/svg+xml,image/webp,image/x-icon,image/vnd.microsoft.icon";

/**
 * "Upload image…": a picture file made into a small icon (hostImageFrom).
 * With one picked, a way to take it away again, back to `look`'s icon.
 */
function ImageChoice({ look, onChange }: { look: HostLook; onChange: (look: HostLook) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const streamer = useSettings((s) => s.streamerMode);
  const image = isHostImage(look.image) ? look.image : null;
  const read = async (file: File | undefined) => {
    if (!file) return;
    setReading(true);
    setError(null);
    try {
      onChange({ icon: look.icon, color: look.color, image: await hostImageFrom(file) });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn't read that picture.");
    } finally {
      setReading(false);
    }
  };
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-1">
        <button
          type="button"
          aria-pressed={!!image}
          title={image ? "Pick another picture" : "A PNG, JPG, SVG, WebP or ICO file, made into a small square icon"}
          disabled={reading}
          onClick={() => input.current?.click()}
          className={cn(
            "flex h-8 min-w-0 flex-1 items-center gap-2 rounded-[7px] px-2 text-left text-[12.5px] text-fg hover:bg-ink/[0.08]",
            image && "bg-ink/[0.12]",
          )}
        >
          {image && !streamer ? (
            <img src={image} alt="" className="size-4 shrink-0 rounded-[22%] object-cover" />
          ) : image ? (
            <ImageIcon size={16} strokeWidth={1.9} className="shrink-0 text-fg-2" />
          ) : (
            <ImagePlus size={16} strokeWidth={1.9} className="shrink-0 text-fg-2" />
          )}
          <span className="min-w-0 flex-1 truncate">{reading ? "Reading…" : image ? "Your picture" : "Upload image…"}</span>
          {image && <Check size={13} strokeWidth={2.2} className="shrink-0" />}
        </button>
        {image && (
          <IconButton type="button" label="Remove the picture" onClick={() => onChange({ icon: look.icon, color: look.color })} className="size-8">
            <X size={14} />
          </IconButton>
        )}
      </div>
      {error && (
        <p role="alert" className="px-2 text-[11.5px] leading-snug text-bad">
          {error}
        </p>
      )}
      <input
        ref={input}
        type="file"
        accept={IMAGE_FILES}
        hidden
        onChange={(e) => {
          void read(e.target.files?.[0]);
          e.target.value = "";
        }}
      />
    </div>
  );
}

const COLOR_NAMES = ["Blue", "Indigo", "Violet", "Magenta", "Cyan", "Gray"];

/**
 * Any color you like for a server, beside the ready ones: a rainbow until
 * you pick one, then that color. `picked` rings it when it's the one in use.
 */
export function OwnHostColor({ color, picked, onChange, size }: { color: string | null; picked: boolean; onChange: (hex: string) => void; size: string }) {
  const own = !!color && !HOST_COLORS.some((c) => c.toLowerCase() === color.toLowerCase());
  return (
    <ColorButton
      label="Your own color"
      value={own ? color : HOST_COLORS[0]}
      onChange={onChange}
      palette={HOST_COLORS.map((c, i) => ({ name: COLOR_NAMES[i] ?? c, color: c }))}
      className={cn("flex shrink-0 items-center justify-center rounded-full", size, own && picked && "ring-2 ring-fg/80 ring-offset-2 ring-offset-[var(--ui-raised)]")}
      style={{ background: own ? color : RAINBOW }}
    >
      {!own && <Plus size={10} strokeWidth={3} className="text-white drop-shadow" />}
    </ColorButton>
  );
}

/**
 * The icon pack, for every server at once, then icons, and unless `colors`
 * is off, a row of colors under them, then "Upload image…". With `auto` or a picture the icons wear `look`'s color but
 * none is marked as picked. Picking an icon or a color drops the picture.
 */
export function LookGrid({
  look,
  onChange,
  colors = true,
  auto = false,
}: {
  look: HostLook;
  onChange: (look: HostLook) => void;
  colors?: boolean;
  auto?: boolean;
}) {
  const picked = !auto && !look.image;
  const pack = useSettings((s) => s.iconPack);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between pl-1" title="The style is the same for every server">
        <span className="text-[11.5px] text-fg-3">Icon style</span>
        <Segmented
          value={pack}
          onChange={(iconPack) => useSettings.getState().update({ iconPack })}
          options={[
            { value: "line", label: "Lines" },
            { value: "pixel", label: "Pixels" },
          ]}
        />
      </div>
      <div className={colors ? "grid grid-cols-6 gap-1" : "flex flex-wrap gap-1"}>
        {Object.entries(HOST_ICONS).map(([id, { icon: I, name }]) => (
          <button
            key={id}
            type="button"
            title={name}
            aria-label={name}
            aria-pressed={picked && look.icon === id}
            onClick={() => onChange({ icon: id, color: look.color })}
            className={cn("flex size-8 items-center justify-center rounded-[7px] hover:bg-ink/[0.08]", picked && look.icon === id && "bg-ink/[0.12]")}
          >
            <I size={18} style={{ color: look.color }} />
          </button>
        ))}
      </div>
      {colors && (
        <div className="flex justify-between px-0.5" role="radiogroup" aria-label="Color">
          {HOST_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              role="radio"
              aria-checked={picked && look.color === c}
              aria-label={`Color ${c}`}
              onClick={() => onChange({ icon: look.icon, color: c })}
              className={cn("size-[16px] rounded-full", picked && look.color === c && "ring-2 ring-fg/80 ring-offset-2 ring-offset-[var(--ui-raised)]")}
              style={{ background: c }}
            />
          ))}
          <OwnHostColor color={look.color} picked={picked} size="size-[16px]" onChange={(c) => onChange({ icon: look.icon, color: c })} />
        </div>
      )}
      <div className="mx-1 h-px bg-ink/[0.07]" />
      <ImageChoice look={look} onChange={onChange} />
    </div>
  );
}

/** "Automatic: website": goes back to SSHeesh's guess, and can look at the server again. */
function AutoChoice({ hostId }: { hostId: string }) {
  const mine = useLabels((s) => s.looks[hostId]);
  const guessed = useLabels((s) => s.autoLooks[hostId]);
  const [checking, setChecking] = useState(false);
  const [failed, setFailed] = useState(false);
  const look = lookOf(hostId, undefined, guessed);
  const Icon = HOST_ICONS[look.icon]?.icon ?? HOST_ICONS.server.icon;
  const what = checking
    ? "checking…"
    : failed
      ? "couldn't check"
      : guessed
        ? (HOST_ICONS[guessed.icon]?.name ?? "Server").toLowerCase()
        : "not checked yet";
  const check = () => {
    setChecking(true);
    setFailed(false);
    void detectRole(hostId).then((role) => {
      setChecking(false);
      setFailed(!role);
    });
  };
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        aria-pressed={!mine}
        onClick={() => useLabels.getState().setLook(hostId, null)}
        className={cn(
          "flex h-8 min-w-0 flex-1 items-center gap-2 rounded-[7px] px-2 text-left text-[12.5px] text-fg hover:bg-ink/[0.08]",
          !mine && "bg-ink/[0.12]",
        )}
      >
        <Icon size={16} strokeWidth={1.9} className="shrink-0" style={{ color: look.color }} />
        <span className="min-w-0 flex-1 truncate">
          Automatic: <span className="text-fg-3">{what}</span>
        </span>
        {!mine && <Check size={13} strokeWidth={2.2} className="shrink-0" />}
      </button>
      <IconButton label={guessed ? "Check the server again" : "Check the server now"} onClick={check} className="size-8">
        <RefreshCw size={13.5} className={cn(checking && "animate-spin")} />
      </IconButton>
    </div>
  );
}

/**
 * Floating picker at a point for a server's look, closed by clicking
 * elsewhere or Escape. It saves by itself and offers "Automatic".
 */
export function LookPopover({ at, hostId, onClose }: { at: { x: number; y: number }; hostId: string; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const mine = useLabels((s) => s.looks[hostId]);
  const guessed = useLabels((s) => s.autoLooks[hostId]);
  useEffect(() => {
    // The own-color picker floats outside it; working in it, or closing it with Esc, keeps this open.
    const inPicker = (e: Event) => !!(e.target as Element | null)?.closest?.("[data-color-picker]");
    const away = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && !inPicker(e) && onClose();
    const esc = (e: KeyboardEvent) => e.key === "Escape" && !document.querySelector("[data-color-picker]") && onClose();
    window.addEventListener("mousedown", away, true);
    window.addEventListener("keydown", esc, true);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("keydown", esc, true);
    };
  }, [onClose]);
  const shown = lookOf(hostId, mine, guessed);
  // Picking an icon or a color in "Automatic" keeps the other half of what's
  // shown. Taking a picture away that you put on "Automatic" goes back to it.
  const pick = (l: HostLook) => {
    const auto = lookOf(hostId, undefined, guessed);
    const backToAuto = mine?.image && !l.image && l.icon === auto.icon && l.color === auto.color;
    useLabels.getState().setLook(hostId, backToAuto ? null : { icon: l.icon, color: l.color, ...(l.image && { image: l.image }) });
  };
  return (
    <div
      ref={ref}
      role="dialog"
      aria-label="Icon and color"
      style={{ left: Math.min(at.x, window.innerWidth - 252), top: Math.min(at.y, window.innerHeight - 300) }}
      className="glass fixed z-50 w-[236px] animate-pop-in rounded-[12px] p-2"
    >
      <AutoChoice hostId={hostId} />
      <div className="mx-1 my-1.5 h-px bg-ink/[0.07]" />
      <LookGrid
        look={{ icon: shown.icon, color: shown.color, image: shown.image }}
        auto={shown.auto}
        onChange={pick}
      />
    </div>
  );
}
