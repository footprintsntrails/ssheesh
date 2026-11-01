// Tokens, keys and passwords in terminal output, found so they can be
// covered on screen. Two kinds: the well-known shapes of tokens (GitHub,
// GitLab, OpenAI, Anthropic, Stripe, Slack, AWS, Google, Telegram bots,
// JWTs...) and values given to names that say they're secret
// (DB_PASSWORD=..., api_key: ..., a password in a URL). Private key blocks
// go by whole lines. The name next to a value stays readable.

import { streamerOn } from "./privacy";
import { useSettings } from "./settings";

export interface Span {
  start: number;
  end: number;
}

/** Tokens recognizable by their shape alone: the whole match is the secret. */
const TOKENS = new RegExp(
  [
    String.raw`\bgh[pousr]_[A-Za-z0-9]{36,}`, // GitHub
    String.raw`\bgithub_pat_[A-Za-z0-9_]{60,}`,
    String.raw`\bglpat-[A-Za-z0-9_-]{20,}`, // GitLab
    String.raw`\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}`, // Anthropic, OpenAI
    String.raw`\b[sr]k_(?:live|test)_[A-Za-z0-9]{16,}`, // Stripe
    String.raw`\bxox[abposr]-[A-Za-z0-9-]{10,}`, // Slack
    String.raw`\bAKIA[0-9A-Z]{16}\b`, // AWS access key
    String.raw`\bAIza[0-9A-Za-z_-]{35}`, // Google
    String.raw`\bhf_[A-Za-z0-9]{30,}`, // Hugging Face
    String.raw`\bnpm_[A-Za-z0-9]{36}`,
    String.raw`\bdo[por]_v1_[a-f0-9]{64}`, // DigitalOcean
    String.raw`\b\d{8,10}:AA[A-Za-z0-9_-]{33}`, // Telegram bot
    String.raw`\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}`, // JWT
  ].join("|"),
  "g",
);

/** A value after `KEY=`, `key: ` or `"key": `: quoted (spaces and all) or up to a space. */
const VALUE = String.raw`\s*[=:]\s*(?:"([^"\n]{4,})"|'([^'\n]{4,})'|([^\s"',;]{4,}))`;

/** Values where only a part of the match is the secret. */
const VALUES = [
  // A name ending in what it holds: DB_PASSWORD, api_key, GITHUB_TOKEN, client-secret.
  new RegExp(
    String.raw`\b[\w.-]*?(?:password|passwd|passphrase|pwd|secret|token|api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key|auth[_-]?key|credentials?)["']?` + VALUE,
    "gid",
  ),
  // A password in a URL: scheme://user:password@host. Streamer mode may have
  // dotted out part of it (a server's user called "postgres"), so • counts too.
  new RegExp(String.raw`(?<![\w+.•-])[a-z•][a-z0-9+.•-]*://[^\s:/@]*:([^\s@/]{3,})@`, "gid"),
  // Authorization headers.
  new RegExp(String.raw`\b(?:Bearer|Basic)\s+([A-Za-z0-9._~+/-]{16,}=*)`, "gd"),
];

/** Values that are settings, not secrets: `PASSWORD=true`, `token: null`, `${TOKEN}`. */
const NOT_SECRET = /^(?:true|false|yes|no|on|off|null|none|nil|undefined|required|optional|disabled|enabled|\*+|•+|x+|<[^>]*>|\$\{?[\w.]+\}?|\{\{.*\}\}|%\w+%)$/i;

/** Where the secrets in one line of text are, sorted and merged. */
export function findSecrets(text: string): Span[] {
  const spans: Span[] = [];
  for (const m of text.matchAll(TOKENS)) spans.push({ start: m.index, end: m.index + m[0].length });
  for (const re of VALUES) {
    for (const m of text.matchAll(re)) {
      const group = [1, 2, 3].find((g) => m[g] !== undefined);
      const at = group && m.indices?.[group];
      if (!at || NOT_SECRET.test(m[group])) continue;
      spans.push({ start: at[0], end: at[1] });
    }
  }
  if (spans.length < 2) return spans;
  spans.sort((a, b) => a.start - b.start);
  const merged: Span[] = [spans[0]];
  for (const s of spans.slice(1)) {
    const last = merged[merged.length - 1];
    if (s.start <= last.end) last.end = Math.max(last.end, s.end);
    else merged.push(s);
  }
  return merged;
}

const KEY_BEGIN = /-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----/;
const KEY_END = /-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----/;
/** A line of a key's body: base64 and nothing else. */
const KEY_BODY = /^\s*[A-Za-z0-9+/]{16,}={0,2}\s*$/;

/** A line that could be part of a key's body, so it's worth looking above the screen for its BEGIN. */
export const looksLikeKeyBody = (line: string) => KEY_BODY.test(line);

/**
 * For each line, whether it's the body of a private key: the lines between
 * BEGIN and END. `inKey` says a key began above the first line.
 */
export function keyLines(lines: string[], inKey = false): boolean[] {
  let open = inKey;
  return lines.map((line) => {
    if (KEY_BEGIN.test(line)) {
      open = !KEY_END.test(line);
      return false;
    }
    if (open && KEY_END.test(line)) {
      open = false;
      return false;
    }
    return open && (KEY_BODY.test(line) || /^[A-Za-z-]+: /.test(line) || !line.trim());
  });
}

/** Whether a key is still open after these lines (it began and hasn't ended). */
export function keyOpenAfter(lines: string[]): boolean {
  let open = false;
  for (const line of lines) {
    if (KEY_BEGIN.test(line)) open = !KEY_END.test(line);
    else if (KEY_END.test(line)) open = false;
  }
  return open;
}

/** The same text with its secrets dotted out, for showing output elsewhere (the "while you were away" card). */
export function maskSecrets(text: string): string {
  const lines = text.split("\n");
  const keys = keyLines(lines);
  return lines
    .map((line, i) => {
      if (keys[i]) return line.replace(/\S/g, "•");
      let out = line;
      for (const s of findSecrets(line).reverse()) out = out.slice(0, s.start) + "•".repeat(s.end - s.start) + out.slice(s.end);
      return out;
    })
    .join("\n");
}

/** Secrets are covered: by your choice, and always while streaming. */
export function secretsHidden(): boolean {
  return useSettings.getState().hideSecrets || streamerOn();
}
