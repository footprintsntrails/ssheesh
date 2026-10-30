// Streamer mode: one place that decides how servers, addresses and people are
// shown. Every label in the app goes through here, so hiding them all is a
// single switch. Terminal output gets the same treatment in output.ts.

import { useEffect } from "react";
import { create } from "zustand";
import { backend } from "./backend";
import { useSettings } from "./settings";
import { hostAddress, useStore } from "./store";
import type { Host, TargetRef } from "./types";

/** Names everyone has. Hiding them protects nothing and makes output hard to read. */
const COMMON = new Set([
  "root", "admin", "administrator", "user", "ubuntu", "debian", "fedora", "centos", "rocky", "alma",
  "pi", "ec2-user", "localhost", "docker", "git", "www-data", "nobody", "home", "server",
]);

interface PrivacyState {
  /** Names seen at run time: server hostnames from stats, this computer's user and name. */
  learned: string[];
  learn(names: (string | undefined)[]): void;
}

export const usePrivacy = create<PrivacyState>((set, get) => ({
  learned: [],
  learn: (names) => {
    const have = new Set(get().learned);
    const add = names.flatMap(variants).filter((n) => !have.has(n));
    if (add.length) set({ learned: [...get().learned, ...new Set(add)] });
  },
}));

/** A name plus its short form: "web-1.example.com" also shows up as "web-1". */
function variants(name: string | undefined): string[] {
  const n = name?.trim();
  if (!n) return [];
  const short = isIp(n) ? "" : n.split(".")[0];
  return short && short !== n ? [n, short] : [n];
}

function isIp(s: string): boolean {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(s) || s.includes(":");
}

function hideable(word: string): boolean {
  return word.length >= 3 && !COMMON.has(word.toLowerCase());
}

export function streamerOn(): boolean {
  return useSettings.getState().streamerMode;
}

export function toggleStreamer() {
  const st = useSettings.getState();
  st.update({ streamerMode: !st.streamerMode });
}

/** "Server 2": stable while the server list stays the same. */
export function hostAlias(h: Host, hosts: Host[]): string {
  const i = hosts.findIndex((x) => x.id === h.id);
  return `Server ${i < 0 ? "?" : i + 1}`;
}

/** Everything that must not appear on screen, longest first. */
let cache: { hosts: Host[]; learned: string[]; words: string[]; re: RegExp | null } | null = null;
function words(): { words: string[]; re: RegExp | null } {
  const hosts = useStore.getState().hosts;
  const learned = usePrivacy.getState().learned;
  if (cache && cache.hosts === hosts && cache.learned === learned) return cache;
  const all = new Set<string>();
  for (const h of hosts) for (const w of [...variants(h.name), ...variants(h.hostname), h.user ?? ""]) if (hideable(w)) all.add(w);
  for (const w of learned) if (hideable(w)) all.add(w);
  const list = [...all].sort((a, b) => b.length - a.length);
  const re = list.length ? new RegExp(list.map(escapeRe).join("|"), "gi") : null;
  cache = { hosts, learned, words: list, re };
  return cache;
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const dots = (s: string) => "•".repeat(s.length);
const IPV4 = /(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?![\w.])/g;
// Colon groups, checked below so clock times like 12:34:56 stay readable.
const IPV6 = /(?<![\w:[;])(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}(?![\w:])/gi;

function looksLikeIpv6(m: string): boolean {
  if (m.length < 6) return false;
  if (m.includes("::")) return /[0-9a-f]{2,}/i.test(m);
  return m.split(":").length >= 5 && /[a-f]/i.test(m);
}

/**
 * Replaces server names, users and IP addresses with dots of the same
 * length, so terminal columns stay where they were.
 */
export function maskText(text: string): string {
  const { re } = words();
  let out = re ? text.replace(re, dots) : text;
  out = out.replace(IPV4, (m) => m.replace(/\d/g, "•"));
  out = out.replace(IPV6, (m) => (looksLikeIpv6(m) ? m.replace(/[0-9a-f]/gi, "•") : m));
  return out;
}

/** How many characters at the end of `text` might be the start of a name or address. */
export function nameTail(text: string): number {
  let hold = 0;
  const lower = text.toLowerCase();
  for (const w of words().words) {
    const lw = w.toLowerCase();
    for (let k = Math.min(lw.length - 1, lower.length); k >= 3; k--) {
      if (lw.startsWith(lower.slice(-k))) {
        hold = Math.max(hold, k);
        break;
      }
    }
  }
  const ip = /[0-9a-f.:]{3,45}$/i.exec(text)?.[0];
  if (ip && /\d/.test(ip) && /[.:]/.test(ip)) hold = Math.max(hold, ip.length);
  return hold;
}

/** A machine's name for places outside React, like system notifications. */
export function targetName(t: TargetRef | null | undefined): string {
  if (!t || t.kind === "local") return "This computer";
  const hosts = useStore.getState().hosts;
  const h = hosts.find((x) => x.id === t.hostId);
  if (!h) return "A server";
  return streamerOn() ? hostAlias(h, hosts) : h.name;
}

/** Labels for servers, addresses and paths, hidden while streamer mode is on. */
export function useNames() {
  const on = useSettings((s) => s.streamerMode);
  const hosts = useStore((s) => s.hosts);
  usePrivacy((s) => s.learned);
  return {
    on,
    host: (h: Host) => (on ? hostAlias(h, hosts) : h.name),
    address: (h: Host) => (on ? "address hidden" : hostAddress(h)),
    target: (t: TargetRef | null | undefined) => {
      if (!t) return "Nothing open";
      if (t.kind === "local") return "This computer";
      const h = hosts.find((x) => x.id === t.hostId);
      if (!h) return "Unknown server";
      return on ? hostAlias(h, hosts) : h.name;
    },
    text: (s: string) => (on ? maskText(s) : s),
  };
}

/** Learns who "This computer" is, once, so its name gets hidden too. */
export function useLearnLocalIdentity() {
  useEffect(() => {
    backend
      .localIdentity()
      .then((id) => usePrivacy.getState().learn([id.user, id.hostname]))
      .catch(() => {});
  }, []);
}
