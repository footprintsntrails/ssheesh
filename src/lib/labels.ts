// Production, Staging and Development labels. A labeled server gets a colored
// frame and tag everywhere it shows up, so you always know which machine
// you're typing into. Also each server's icon and color: the ones you picked
// (or a picture of your own), else what SSHeesh guessed it's for (roles.ts)
// in a color of its own. Saved on this computer, per server.

import {
  Bot,
  Box,
  Camera,
  Cloud,
  Cpu,
  Database,
  Film,
  FlaskConical,
  Gamepad2,
  Globe,
  HardDrive,
  House,
  Laptop,
  Music,
  Rocket,
  Router,
  Server,
  Shield,
  type LucideIcon,
} from "lucide-react";
import { useMemo } from "react";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { quietStorage } from "./persist";
import { useSettings } from "./settings";
import { useStore } from "./store";
import type { Host, TargetRef } from "./types";

export type EnvLabel = "prod" | "staging" | "dev";

/** Colors come from the theme's status colors, so they read well on light and dark. */
export const ENV_LABELS: Record<EnvLabel, { name: string; short: string; text: string; tint: string; ring: string; dot: string; bar: string }> = {
  prod: { name: "Production", short: "Prod", text: "text-bad", tint: "bg-bad/15", ring: "ring-bad/40", dot: "fill-bad text-bad", bar: "bg-bad" },
  staging: { name: "Staging", short: "Staging", text: "text-warn", tint: "bg-warn/15", ring: "ring-warn/35", dot: "fill-warn text-warn", bar: "bg-warn" },
  dev: { name: "Development", short: "Dev", text: "text-ok", tint: "bg-ok/15", ring: "ring-ok/30", dot: "fill-ok text-ok", bar: "bg-ok" },
};

export const ENV_ORDER: EnvLabel[] = ["prod", "staging", "dev"];

/** Icons you can give a server, so it's easy to spot, also in the narrow sidebar. */
export const HOST_ICONS: Record<string, { icon: LucideIcon; name: string }> = {
  server: { icon: Server, name: "Server" },
  cloud: { icon: Cloud, name: "Cloud" },
  globe: { icon: Globe, name: "Website" },
  database: { icon: Database, name: "Database" },
  box: { icon: Box, name: "Containers" },
  home: { icon: House, name: "Home" },
  router: { icon: Router, name: "Network" },
  drive: { icon: HardDrive, name: "Storage" },
  cpu: { icon: Cpu, name: "Compute" },
  shield: { icon: Shield, name: "Security" },
  rocket: { icon: Rocket, name: "Launch" },
  flask: { icon: FlaskConical, name: "Lab" },
  bot: { icon: Bot, name: "Bot" },
  game: { icon: Gamepad2, name: "Game server" },
  film: { icon: Film, name: "Media" },
  music: { icon: Music, name: "Music" },
  camera: { icon: Camera, name: "Cameras" },
  laptop: { icon: Laptop, name: "Computer" },
};

/**
 * Icon colors: blue, indigo, violet, magenta, cyan and gray. Muted enough to
 * sit next to each other in a list, and readable on every theme: 3:1 or more
 * on Daylight's white, 4:1 or more on the dark ones. Never red, orange,
 * yellow or green: those say how a server is doing. Looks saved with an
 * older color keep it.
 */
export const HOST_COLORS = ["#2389e2", "#7577e3", "#9d69d2", "#ba5db3", "#0997b4", "#7e8590"];

/** The colors servers get before you pick one. Not gray: it looks switched off. */
const OWN_COLORS = HOST_COLORS.slice(0, -1);

/** A server's own color, the same every time, so servers differ at a glance. */
export function hostColor(hostId: string): string {
  let h = 0;
  for (let i = 0; i < hostId.length; i++) h = (Math.imul(h, 31) + hostId.charCodeAt(i)) | 0;
  return OWN_COLORS[(h >>> 0) % OWN_COLORS.length];
}

export interface HostLook {
  icon: string;
  color: string;
  /** Your own picture, shown instead of the icon: a small PNG data URL from hostImageFrom. */
  image?: string;
}

/** Sizes a picture is saved at, largest first: the first that fits under MAX_IMAGE_CHARS wins. */
export const IMAGE_SIZES = [64, 48, 32];

/** Longest picture kept, as a data URL: a few dozen servers' pictures still fit in local storage. */
export const MAX_IMAGE_CHARS = 16_000;

/** Bigger files aren't worth decoding for a 64-pixel icon. */
const MAX_IMAGE_FILE = 10 * 1024 * 1024;

/** The middle square of a w × h picture, so it fills the icon without stretching. */
export function coverSquare(w: number, h: number): { x: number; y: number; side: number } {
  const side = Math.min(w, h);
  return { x: (w - side) / 2, y: (h - side) / 2, side };
}

/** The first of `render`'s results, largest size first, that's short enough; null if none is. */
export function firstThatFits(render: (size: number) => string, sizes = IMAGE_SIZES, max = MAX_IMAGE_CHARS): string | null {
  for (const size of sizes) {
    const url = render(size);
    if (url.length <= max) return url;
  }
  return null;
}

/** Only small PNG data URLs, as hostImageFrom makes them, are ever shown. */
export function isHostImage(s: unknown): s is string {
  return typeof s === "string" && s.startsWith("data:image/png;base64,") && s.length <= MAX_IMAGE_CHARS;
}

/**
 * A picture file (PNG, JPG, SVG, WebP, ICO) as a server's icon: its middle
 * square, scaled down to a small PNG with its transparency, as a data URL.
 * SVGs are only drawn as images, so nothing in them runs. Neither the file
 * nor its path is kept.
 */
export async function hostImageFrom(file: File): Promise<string> {
  if (file.size > MAX_IMAGE_FILE) throw new Error("That file is too big. Pick one under 10 MB.");
  const url = URL.createObjectURL(file);
  try {
    const img = new Image();
    img.src = url;
    await img.decode().catch(() => {
      throw new Error("Couldn't read that picture. Try a PNG, JPG, SVG, WebP or ICO.");
    });
    // First the middle square at up to 256 pixels, then each size from that:
    // two steps scale down more smoothly. The whole picture is drawn, shifted,
    // because Chromium crops an SVG without a width and height wrongly.
    const w = img.naturalWidth || 256;
    const h = img.naturalHeight || 256;
    const crop = coverSquare(w, h);
    const square = canvasOf(Math.min(crop.side, 256));
    const k = square.canvas.width / crop.side;
    square.g.drawImage(img, -crop.x * k, -crop.y * k, w * k, h * k);
    const fit = firstThatFits((size) => {
      const icon = canvasOf(size);
      icon.g.drawImage(square.canvas, 0, 0, size, size);
      return icon.canvas.toDataURL("image/png");
    });
    if (!fit) throw new Error("That picture has too much detail for an icon. Try a simpler one.");
    return fit;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** A transparent square canvas, set to scale smoothly. */
function canvasOf(size: number): { canvas: HTMLCanvasElement; g: CanvasRenderingContext2D } {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = Math.max(1, Math.round(size));
  const g = canvas.getContext("2d");
  if (!g) throw new Error("Couldn't draw that picture.");
  g.imageSmoothingQuality = "high";
  return { canvas, g };
}

interface LabelsState {
  env: Record<string, EnvLabel>;
  setEnv(hostId: string, env: EnvLabel | null): void;
  /** The looks you picked. */
  looks: Record<string, HostLook>;
  /** Null goes back to the guessed look. */
  setLook(hostId: string, look: HostLook | null): void;
  /** Icons SSHeesh guessed from what runs on each server (roles.ts). Yours in `looks` win. */
  autoLooks: Record<string, { icon: string }>;
  setAutoLook(hostId: string, icon: string): void;
  /** Servers from ~/.ssh/config you'd rather not see in the list. */
  hidden: string[];
  setHidden(hostId: string, hidden: boolean): void;
  showAll(): void;
  /** Servers from ~/.ssh/config you've already said yes or no to. */
  configSeen: string[];
  /** Records your choice for some ~/.ssh/config servers: the rest of `ids` stay hidden. */
  chooseConfig(ids: string[], keep: string[]): void;
}

export const useLabels = create<LabelsState>()(
  persist(
    (set) => ({
      env: {},
      setEnv: (hostId, env) =>
        set((s) => {
          const next = { ...s.env };
          if (env) next[hostId] = env;
          else delete next[hostId];
          return { env: next };
        }),
      hidden: [],
      setHidden: (hostId, hidden) =>
        set((s) => ({ hidden: hidden ? [...new Set([...s.hidden, hostId])] : s.hidden.filter((h) => h !== hostId) })),
      showAll: () => set({ hidden: [] }),
      configSeen: [],
      chooseConfig: (ids, keep) =>
        set((s) => ({
          configSeen: [...new Set([...s.configSeen, ...ids])],
          hidden: [...new Set([...s.hidden.filter((h) => !ids.includes(h)), ...ids.filter((id) => !keep.includes(id))])],
        })),
      looks: {},
      setLook: (hostId, look) =>
        set((s) => {
          const next = { ...s.looks };
          if (look) next[hostId] = look;
          else delete next[hostId];
          return { looks: next };
        }),
      autoLooks: {},
      setAutoLook: (hostId, icon) => set((s) => ({ autoLooks: { ...s.autoLooks, [hostId]: { icon } } })),
    }),
    {
      name: "sshee-labels",
      storage: quietStorage(),
      // 2 added autoLooks, which start empty. Before it, adding a server saved
      // the default look (server icon, old blue) as if you'd picked it; those
      // go, so the server gets a detected icon and a color of its own.
      version: 2,
      migrate: (saved, version) => {
        const s = saved as LabelsState;
        if (version < 2 && s.looks) {
          s.looks = Object.fromEntries(Object.entries(s.looks).filter(([, l]) => !(l.icon === "server" && l.color.toLowerCase() === "#5b9dff")));
        }
        return s;
      },
    },
  ),
);

/** The label of the server a pane or row points at, if any. */
export function useEnv(target: TargetRef | null | undefined): EnvLabel | null {
  return useLabels((s) => (target?.kind === "host" ? (s.env[target.hostId] ?? null) : null));
}

/** The look a server shows: yours, else the guessed icon (or the plain server one) in the server's own color. */
export function lookOf(
  hostId: string,
  mine: HostLook | undefined,
  guessed: { icon: string } | undefined,
): HostLook & { auto: boolean } {
  return mine ? { ...mine, auto: false } : { icon: guessed?.icon ?? "server", color: hostColor(hostId), auto: true };
}

/**
 * A server's icon and color: the ones you picked, else what SSHeesh guessed
 * it's for in a color of its own. `id` is the HOST_ICONS key; `auto` is true
 * when you haven't picked. No color without a server. `image` is your own
 * picture, if any; streamer mode leaves it out, as it could give the server away.
 */
export function useHostLook(hostId: string | undefined): { icon: LucideIcon; color: string | null; id: string; auto: boolean; image: string | null } {
  const mine = useLabels((s) => (hostId ? s.looks[hostId] : undefined));
  const guessed = useLabels((s) => (hostId ? s.autoLooks[hostId] : undefined));
  const streamer = useSettings((s) => s.streamerMode);
  if (!hostId) return { icon: Server, color: null, id: "server", auto: true, image: null };
  const look = lookOf(hostId, mine, guessed);
  const image = !streamer && isHostImage(look.image) ? look.image : null;
  return { icon: HOST_ICONS[look.icon]?.icon ?? Server, color: look.color, id: look.icon, auto: look.auto, image };
}

/**
 * The server list without the ones you hid, and without ~/.ssh/config
 * servers you haven't said yes to yet.
 */
export function useVisibleHosts(): Host[] {
  const hosts = useStore((s) => s.hosts);
  const hidden = useLabels((s) => s.hidden);
  const seen = useLabels((s) => s.configSeen);
  return useMemo(
    () => hosts.filter((h) => !hidden.includes(h.id) && (h.source !== "sshConfig" || seen.includes(h.id))),
    [hosts, hidden, seen],
  );
}

/** ~/.ssh/config servers you haven't decided about yet. */
export function useUndecidedConfig(): Host[] {
  const hosts = useStore((s) => s.hosts);
  const seen = useLabels((s) => s.configSeen);
  return useMemo(() => hosts.filter((h) => h.source === "sshConfig" && !seen.includes(h.id)), [hosts, seen]);
}
