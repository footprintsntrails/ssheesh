// Understands whatever people paste as a server address: "root@1.2.3.4",
// "host:2222", "ssh://user@host", or a whole "ssh -p 2222 -i key user@host".

export interface ParsedAddress {
  host: string;
  user: string | null;
  port: string | null;
  identityFile: string | null;
}

function words(s: string): string[] {
  const out: string[] = [];
  for (const m of s.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

/** "user@host:port", "[v6]:port", "ssh://user@host:port" → parts. */
function splitTarget(target: string): Omit<ParsedAddress, "identityFile"> {
  let t = target.replace(/^ssh:\/\//, "").replace(/\/$/, "");
  let user: string | null = null;
  const at = t.lastIndexOf("@");
  if (at > 0) {
    user = t.slice(0, at);
    t = t.slice(at + 1);
  }
  let port: string | null = null;
  const v6 = /^\[(.+)\](?::(\d+))?$/.exec(t);
  if (v6) return { host: v6[1], user, port: v6[2] ?? null };
  const hp = /^([^:]+):(\d+)$/.exec(t);
  if (hp) {
    t = hp[1];
    port = hp[2];
  }
  return { host: t, user, port };
}

export function parseAddress(input: string): ParsedAddress {
  const text = input.trim();
  const parts = words(text);
  if (parts[0] !== "ssh") return { ...splitTarget(text), identityFile: null };
  let user: string | null = null;
  let port: string | null = null;
  let identityFile: string | null = null;
  let target = "";
  // Options that take a value; the rest are flags.
  const withValue = new Set(["-p", "-i", "-l", "-o", "-J", "-F", "-L", "-R", "-D", "-W", "-b", "-c", "-E", "-m", "-O", "-Q", "-S", "-w", "-B", "-e", "-I"]);
  for (let i = 1; i < parts.length; i++) {
    const w = parts[i];
    if (w.startsWith("-") && w.length > 1) {
      const flag = w.slice(0, 2);
      const glued = w.length > 2 ? w.slice(2) : null;
      if (!withValue.has(flag)) continue;
      const value = glued ?? parts[++i] ?? "";
      if (flag === "-p") port = value;
      else if (flag === "-i") identityFile = value;
      else if (flag === "-l") user = value;
      continue;
    }
    if (!target) target = w;
    else break; // the rest is a remote command
  }
  const t = splitTarget(target);
  return { host: t.host, user: t.user ?? user, port: t.port ?? port, identityFile };
}
