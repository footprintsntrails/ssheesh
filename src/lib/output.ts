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

/** Output this long is a screenful or more on its way. */
const BURST_SIZE = 2048;
/** Erasing the screen: what draws it again is right behind. */
const ERASES = /\x1b\[[023]?J/;
/** A burst is over when nothing has come for this long: a little over the 16 ms a steady stream arrives in. */
const BURST_QUIET_MS = 24;
/** The longest the old picture stays while output keeps coming, so a flood still moves. */
const BURST_MAX_MS = 64;
/** The same after an erase: better a moment's wait than an empty screen. The pieces of a redraw can be a round trip to the server apart. */
const ERASED_QUIET_MS = 80;
const ERASED_MAX_MS = 200;
/** What you type answers at once: an erase this soon after a key is a prompt redrawing. */
const TYPED_MS = 150;

const SYNC_ON = "\x1b[?2026h";
const SYNC_OFF = "\x1b[?2026l";
/** Private modes being set or reset, in any way of writing them: the terminal reads `?25;2026h` and `?02026h` too. */
const PRIVATE_MODES = /(?:\x1b\[|\x9b)\?([\d;:]*)([hl])/g;
/** How long the terminal itself holds for a program that never ends its hold. */
const PROGRAM_HOLD_MS = 1000;

/** How much of `s`'s end could be the start of a synchronized-output mark cut off by a read. */
function cutSyncMark(s: string): number {
  for (let n = Math.min(SYNC_ON.length - 1, s.length); n > 0; n--) {
    if (s.endsWith(SYNC_ON.slice(0, n))) return n;
  }
  return 0;
}

/**
 * Shows a burst of output as one picture. A program redrawing its screen
 * (an agent erasing everything and printing it again, say) reaches us in
 * several pieces over the network, and drawing each as it comes shows an
 * empty or half-drawn screen: a terminal that flickers and tears. So while
 * a burst arrives the terminal keeps the old picture, using its synchronized
 * output mode (DEC 2026), and draws once the burst is over.
 *
 * Programs use that mode too, and tmux around every update, which would end
 * the wait early. So it's all kept here: the terminal is told to wait while
 * a burst is arriving or a program asked for it, and their marks go no further.
 */
export class FrameGate {
  /** Holding the picture for a burst. */
  private holding = false;
  /** The burst began by erasing the screen. */
  private erased = false;
  /** A program asked to hold it (its own mark, not yet ended). */
  private programHolds = false;
  /** What's been said to the terminal. */
  private told = false;
  private cut = "";
  private quiet: ReturnType<typeof setTimeout> | undefined;
  private longest: ReturnType<typeof setTimeout> | undefined;
  private programHold: ReturnType<typeof setTimeout> | undefined;
  private typedAt = 0;

  constructor(private readonly write: (text: string) => void) {}

  /** You pressed a key: what comes back shows at once. */
  typed() {
    this.typedAt = Date.now();
  }

  push(chunk: string) {
    let text = this.cut + chunk;
    this.cut = "";
    const keep = cutSyncMark(text);
    if (keep) {
      this.cut = text.slice(-keep);
      text = text.slice(0, -keep);
    }
    text = text.replace(PRIVATE_MODES, (whole, modes: string, on: string) => {
      const list = modes.split(";");
      const others = list.filter((m) => Number(m) !== 2026);
      if (others.length === list.length) return whole;
      this.programHolds = on === "h";
      clearTimeout(this.programHold);
      if (this.programHolds) this.programHold = setTimeout(() => this.programGaveUp(), PROGRAM_HOLD_MS);
      return others.length ? whole.replace(modes, others.join(";")) : "";
    });
    const erases = ERASES.test(text) && Date.now() - this.typedAt > TYPED_MS;
    if (!this.holding && (erases || text.length >= BURST_SIZE)) {
      this.holding = true;
      this.longest = setTimeout(() => this.release(), erases ? ERASED_MAX_MS : BURST_MAX_MS);
    } else if (this.holding && erases && !this.erased) {
      // A flood that turns into a redraw gets the longer wait.
      clearTimeout(this.longest);
      this.longest = setTimeout(() => this.release(), ERASED_MAX_MS);
    }
    this.erased ||= erases;
    // Told to wait before the text, and to draw only after it.
    if (this.holding || this.programHolds) this.tell();
    if (text) this.write(text);
    this.tell();
    clearTimeout(this.quiet);
    if (this.holding || this.cut) this.quiet = setTimeout(() => this.release(), this.erased ? ERASED_QUIET_MS : BURST_QUIET_MS);
  }

  /** The burst is over, or has gone on long enough: draw what's there. */
  release() {
    clearTimeout(this.quiet);
    clearTimeout(this.longest);
    this.holding = false;
    this.erased = false;
    if (this.cut) {
      const rest = this.cut;
      this.cut = "";
      this.write(rest);
    }
    this.tell();
  }

  /** The connection dropped: whatever program asked to hold the picture is gone. */
  reset() {
    this.programHolds = false;
    clearTimeout(this.programHold);
    this.release();
  }

  dispose() {
    clearTimeout(this.quiet);
    clearTimeout(this.longest);
    clearTimeout(this.programHold);
  }

  /** A program never ended its hold, and the terminal has stopped waiting by now: keep in step. */
  private programGaveUp() {
    this.programHolds = false;
    if (!this.holding) this.told = false;
  }

  private tell() {
    const hold = this.holding || this.programHolds;
    if (hold === this.told) return;
    this.told = hold;
    this.write(hold ? SYNC_ON : SYNC_OFF);
  }
}
