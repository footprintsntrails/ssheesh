// Terminal output passes through here on its way to the screen. Names get
// hidden in streamer mode and errors and warnings get colored. Both only
// touch plain text: escape sequences (colors, cursor moves, titles) pass
// through untouched, so full-screen apps and tmux keep drawing correctly.

import { findMatches, highlightOn, highlightTail } from "./highlight";
import { maskText, nameTail, streamerOn } from "./privacy";

type Token = { kind: "text" | "esc"; s: string };

/** Escape sequences longer than this are passed on as they are. */
const MAX_ESC = 4096;

/**
 * Splits output into plain text and escape sequences. An escape sequence cut
 * off at the end comes back as `rest`, to be joined with the next chunk.
 */
export function tokenize(input: string): { tokens: Token[]; rest: string } {
  const tokens: Token[] = [];
  let i = 0;
  while (i < input.length) {
    const esc = input.indexOf("\x1b", i);
    if (esc === -1) {
      tokens.push({ kind: "text", s: input.slice(i) });
      break;
    }
    if (esc > i) tokens.push({ kind: "text", s: input.slice(i, esc) });
    const end = escapeEnd(input, esc);
    if (end === -1) {
      if (input.length - esc > MAX_ESC) {
        tokens.push({ kind: "esc", s: input.slice(esc) });
        return { tokens, rest: "" };
      }
      return { tokens, rest: input.slice(esc) };
    }
    tokens.push({ kind: "esc", s: input.slice(esc, end) });
    i = end;
  }
  return { tokens, rest: "" };
}

/** Index just past the escape sequence starting at `at`, or -1 if it's cut off. */
function escapeEnd(s: string, at: number): number {
  const kind = s[at + 1];
  if (kind === undefined) return -1;
  if (kind === "[") {
    // CSI: parameters and intermediates, then one final byte.
    for (let j = at + 2; j < s.length; j++) {
      const c = s.charCodeAt(j);
      if (c >= 0x40 && c <= 0x7e) return j + 1;
      if (c < 0x20 || c > 0x3f) return j; // malformed: stop here
    }
    return -1;
  }
  if (kind === "]" || kind === "P" || kind === "_" || kind === "^" || kind === "X") {
    // OSC, DCS and friends: up to BEL (OSC only) or ESC \.
    for (let j = at + 2; j < s.length; j++) {
      if (kind === "]" && s[j] === "\x07") return j + 1;
      if (s[j] === "\x1b") return s[j + 1] === "\\" ? j + 2 : s[j + 1] === undefined ? -1 : j;
    }
    return -1;
  }
  // ESC, optional intermediates, one final byte: ESC 7, ESC ( B, ...
  let j = at + 1;
  while (j < s.length && s.charCodeAt(j) >= 0x20 && s.charCodeAt(j) <= 0x2f) j++;
  return j < s.length ? j + 1 : -1;
}

/**
 * One terminal's output filter. It remembers the current text style so a
 * highlighted word can hand the color back afterwards, and it holds back a
 * chunk's last few characters for up to 30 ms when they could be the start
 * of a name or a highlighted word.
 */
export class OutputPipeline {
  private carry = "";
  private timer: ReturnType<typeof setTimeout> | undefined;
  /** SGR parameters since the last reset, replayed after a highlight. */
  private style: string[] = [];

  /**
   * `ownScreen`: a program that draws its own screen is running (an agent,
   * vim). It redraws lines in pieces, so a highlight would land on half an
   * address one time and not the next, mixed into its own colors.
   */
  constructor(
    private readonly emit: (text: string) => void,
    private readonly ownScreen: () => boolean = () => false,
  ) {}

  push(chunk: string) {
    clearTimeout(this.timer);
    const mask = streamerOn();
    const highlight = highlightOn() && !this.ownScreen();
    if (!mask && !highlight) {
      this.flush();
      this.trackStyle(chunk);
      this.emit(chunk);
      return;
    }
    const { tokens, rest } = tokenize(this.carry + chunk);
    this.carry = rest;
    const last = tokens[tokens.length - 1];
    if (!rest && last?.kind === "text") {
      const hold = Math.max(mask ? nameTail(last.s) : 0, highlight ? highlightTail(last.s) : 0);
      if (hold) {
        this.carry = last.s.slice(-hold);
        last.s = last.s.slice(0, -hold);
      }
    }
    if (this.carry) this.timer = setTimeout(() => this.flush(), 30);
    const out = this.render(tokens, mask, highlight);
    if (out) this.emit(out);
  }

  /** Lets anything held back through now. */
  flush() {
    clearTimeout(this.timer);
    if (!this.carry) return;
    const rest = this.carry;
    this.carry = "";
    if (rest.startsWith("\x1b")) return this.emit(rest);
    this.emit(this.render([{ kind: "text", s: rest }], streamerOn(), highlightOn() && !this.ownScreen()));
  }

  private render(tokens: Token[], mask: boolean, highlight: boolean): string {
    let out = "";
    for (const t of tokens) {
      if (t.kind === "esc") {
        this.trackStyle(t.s);
        out += t.s;
        continue;
      }
      const text = mask ? maskText(t.s) : t.s;
      out += highlight ? this.paint(text) : text;
    }
    return out;
  }

  private paint(text: string): string {
    const matches = findMatches(text);
    if (!matches.length) return text;
    const restore = `\x1b[0m${this.style.length ? `\x1b[${this.style.join(";")}m` : ""}`;
    let out = "";
    let at = 0;
    for (const m of matches) {
      out += text.slice(at, m.start) + `\x1b[${m.sgr}m` + text.slice(m.start, m.end) + restore;
      at = m.end;
    }
    return out + text.slice(at);
  }

  /**
   * Follows SGR (text style) sequences, `ESC [ params m`, so the style can be
   * put back. Every chunk passes through here, so it's a plain scan rather
   * than a regex.
   */
  private trackStyle(s: string) {
    for (let i = s.indexOf("\x1b["); i !== -1; i = s.indexOf("\x1b[", i)) {
      let j = i + 2;
      // Digits, ":" and ";".
      while (j < s.length && s.charCodeAt(j) >= 0x30 && s.charCodeAt(j) <= 0x3b) j++;
      if (s.charCodeAt(j) !== 0x6d /* m */) {
        i = j;
        continue;
      }
      const p = s.slice(i + 2, j);
      if (p === "" || p === "0") this.style = [];
      else if (p.startsWith("0;")) this.style = [p.slice(2)];
      else {
        this.style.push(p);
        if (this.style.length > 24) this.style.shift();
      }
      i = j + 1;
    }
  }
}
