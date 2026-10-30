// Production, Staging and Development labels. A labeled server gets a colored
// frame and tag everywhere it shows up, so you always know which machine
// you're typing into. Saved on this computer, per server.

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

/** Icon colors. Muted enough to sit next to each other in a list. */
export const HOST_COLORS = ["#5b9dff", "#8b8fff", "#b07cff", "#ff6fb1", "#ff7a6b", "#ff9f43", "#f5c451", "#3dd68c", "#2bc4b4", "#9aa3ad"];

export interface HostLook {
  icon: string;
  color: string;
}

interface LabelsState {
  env: Record<string, EnvLabel>;
  setEnv(hostId: string, env: EnvLabel | null): void;
  looks: Record<string, HostLook>;
  setLook(hostId: string, look: HostLook | null): void;
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
    }),
    { name: "sshee-labels", version: 1 },
  ),
);

/** The label of the server a pane or row points at, if any. */
export function useEnv(target: TargetRef | null | undefined): EnvLabel | null {
  return useLabels((s) => (target?.kind === "host" ? (s.env[target.hostId] ?? null) : null));
}

/** A server's icon and color; the plain server icon in the accent color if you never picked one. */
export function useHostLook(hostId: string | undefined): { icon: LucideIcon; color: string | null } {
  const look = useLabels((s) => (hostId ? s.looks[hostId] : undefined));
  return { icon: HOST_ICONS[look?.icon ?? ""]?.icon ?? Server, color: look?.color ?? null };
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
