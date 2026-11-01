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

/** Attributes that turn off together, by the SGR parameter that turns them off. */
const ATTRIBUTE_OFF: Record<number, number[]> = { 22: [1, 2], 23: [3], 24: [4], 25: [5], 27: [7], 28: [8], 29: [9], 55: [53] };

/**
 * The text style in effect, folded from SGR sequences so it can be set again
 * in one short sequence. Replaying every sequence since the last reset ran
 * past the 32 parameters a terminal reads in one, after a colorful logo say,
 * and then a background long switched off came back.
 */
export class TextStyle {
  private fg = "";
  private bg = "";
  private underlineColor = "";
  /** Bold, italic and so on, by the parameter that turns each on (4 also holds 21 and 4:3). */
  private attributes = new Map<number, string>();

  /** Takes in one SGR sequence's parameters, "1;38;2;255;0;0" for `ESC [ 1;38;2;255;0;0 m`. */
  apply(params: string) {
    const ps = params.split(";");
    for (let k = 0; k < ps.length; k++) {
      const p = ps[k];
      const colon = p.indexOf(":");
      const n = Number(colon === -1 ? p : p.slice(0, colon));
      if (n === 0) this.reset();
      else if (n === 38 || n === 48 || n === 58) {
        // 38:2::r:g:b carries its color; 38;5;n and 38;2;r;g;b take the next parameters.
        const take = colon !== -1 ? 0 : ps[k + 1] === "5" ? 2 : ps[k + 1] === "2" ? 4 : -1;
        if (take === -1 || k + take >= ps.length) break;
        const color = ps.slice(k, k + take + 1).join(";");
        k += take;
        if (n === 38) this.fg = color;
        else if (n === 48) this.bg = color;
        else this.underlineColor = color;
      } else if ((n >= 30 && n <= 37) || (n >= 90 && n <= 97)) this.fg = p;
      else if ((n >= 40 && n <= 47) || (n >= 100 && n <= 107)) this.bg = p;
      else if (n === 39) this.fg = "";
      else if (n === 49) this.bg = "";
      else if (n === 59) this.underlineColor = "";
      else if (n === 4 && p === "4:0") this.attributes.delete(4);
      else if (n === 21 || n === 4) this.attributes.set(4, p);
      else if (n === 6) this.attributes.set(5, p);
      else if ((n >= 1 && n <= 9) || n === 53) this.attributes.set(n, p);
      else for (const a of ATTRIBUTE_OFF[n] ?? []) this.attributes.delete(a);
    }
  }

  /** The parameters that set this style again after a reset, "" for the plain one. */
  sgr(): string {
    return [...this.attributes.values(), this.fg, this.bg, this.underlineColor].filter(Boolean).join(";");
  }

  private reset() {
    this.fg = this.bg = this.underlineColor = "";
    this.attributes.clear();
  }
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
  /** The style in effect, set again after a highlight. */
  private style = new TextStyle();

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
    const style = this.style.sgr();
    const restore = `\x1b[0m${style ? `\x1b[${style}m` : ""}`;
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
      this.style.apply(s.slice(i + 2, j));
      i = j + 1;
    }
  }
}
