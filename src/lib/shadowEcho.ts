// Stage zero of showing typed text before the server answers: nothing is
// shown. For every key this works out whether it could have been drawn at
// once and whether that guess would have been right, from the screen's cells
// and not the bytes that drew them (those get rewritten on the way, by tmux
// and by output.ts). All it does with the answer is count. Over Tor a key takes
// 60-400 ms to come back, and the counts say whether drawing keys early would
// be worth building at all.
//
// It reads the terminal and calls `EchoSink`, and it imports nothing: it has
// no way to draw, send or save anything.
//
// Typed text isn't kept. To compare a key with its cell, a salted fingerprint
// of the key is held until its answer comes, or for about 3 seconds if none
// does. That's no lock against someone reading this program's memory, who could
// try every letter (and would see the key go past anyway). It means no typed
// text can end up in the counts, on disk or in a log by accident.
//
// The rule for "could have been drawn" is the best a client can do alone: the
// echo of an earlier key landed where it should, on this line, since the last
// Enter or other control key. That's mosh's rule, and it has a hole: a password
// prompt can appear on a row that already echoed something, and keys typed
// then count as drawable. They come out here as `wrong`, `dropped` or `silent`,
// which is how the hole gets measured. Real characters on screen would also
// need the server to announce its prompts.

/** What the guesses read of a terminal. xterm's `Terminal` fits this as it is, and nothing in it can change anything. */
export interface EchoScreen {
  readonly cols: number;
  readonly buffer: {
    readonly active: {
      readonly cursorX: number;
      readonly cursorY: number;
      readonly baseY: number;
      getLine(y: number): { getCell(x: number): { getChars(): string } | undefined } | undefined;
    };
  };
}

/** Where a key went: a line being typed, an agent, or a program with a screen of its own. */
export type EchoPlace = "line" | "agent" | "screen";

/**
 * What gets counted.
 * - `keys`: every key, Enter and arrows too; not pastes, clicks or the terminal's own replies.
 * - `typed`: characters and Backspace, what could be drawn at all.
 * - `atOnce`: of those, could have been drawn at once. Each ends as `right`; `wrong`; `dropped`, on its way when something else proved the guesses wrong;
 *   `silent`, nothing came back; or `unjudged`, when an Enter or a new size came first and there's no telling.
 * - `atPrompt`: of `atOnce`, on a row that looked like a shell prompt. A rough stand-in for a prompt the server announces.
 * - `waited`: had to wait for its echo: the first on a line, or the gate was shut.
 * - `heldSilent`: of `waited`, nothing ever came back. What the gate protected: password prompts, and vim's commands too.
 * - `midLine`, `unfit`: not guessed, because of text to the right of the cursor; or a wide character, or the last column.
 * - `passwordOpen`: the gate opened on a row that reads like a password prompt. Should stay at zero.
 *
 * Keys typed on such a row aren't counted as such: saved, that would be a password's length.
 */
export type EchoCount =
  | "keys"
  | "typed"
  | "atOnce"
  | "atPrompt"
  | "right"
  | "wrong"
  | "dropped"
  | "silent"
  | "unjudged"
  | "waited"
  | "heldSilent"
  | "midLine"
  | "unfit"
  | "passwordOpen";

export interface EchoSink {
  count(place: EchoPlace, what: EchoCount): void;
  /** How long a key's echo took. */
  echoTook(place: EchoPlace, ms: number): void;
}

/** How long a key's answer is waited for. Nothing is drawn, so this only decides when a key counts as `silent`. */
export const GIVE_UP_MS = 3000;
/** A flood of keys (a stuck key, say) isn't typing. */
const MOST_ON_THE_WAY = 128;

type KeyKind = "none" | "break" | "other" | "soft" | "erase" | "char";

/** The terminal's own answers to a server's questions, and focus reports. They aren't keys. */
const NOT_KEYS = /^\x1b(?:[\]P_^X].|\[[?>=]|\[\d*(?:;\d+)*[Rn]$|\[[IO]$)/;
/** Mouse reports and pastes: a click may switch tmux's pane. */
const BREAKS = /^\x1b\[(?:<|M|200~)/;
/** Left, Right, Home and End, in both ways of writing them; Tab; Ctrl+A, E, K, U and W. They edit the line but don't end it. */
const SOFT_KEYS = /^(?:\x1b[[O][DCHF]|[\t\x01\x05\x0b\x15\x17])$/;

function keyKind(data: string): KeyKind {
  if (NOT_KEYS.test(data)) return "none";
  if (BREAKS.test(data)) return "break";
  if (SOFT_KEYS.test(data)) return "soft";
  if (data === "\x7f" || data === "\b") return "erase";
  const first = data.codePointAt(0) ?? 0;
  if (first === 0x1b || first < 0x20) return "other";
  // Text that came in one piece: an input method, or a paste with no marks around it.
  return String.fromCodePoint(first).length === data.length ? "char" : "break";
}

/**
 * Takes one cell, by what people type: Latin, Greek, Cyrillic, their
 * punctuation, currency signs and №. Everything else (wide characters, emoji,
 * marks joining the one before) isn't guessed, because xterm doesn't say how
 * wide a character is until it's on screen.
 */
function oneCell(c: number): boolean {
  return (
    (c >= 0x20 && c < 0x7f) ||
    (c >= 0xa0 && c <= 0x2ff) ||
    (c >= 0x370 && c <= 0x52f) ||
    (c >= 0x2010 && c <= 0x2027) ||
    (c >= 0x2030 && c <= 0x205e) ||
    (c >= 0x20a0 && c <= 0x20bf) ||
    c === 0x2116
  );
}

/** A shell's prompt ends like this. Only a rough guess, for counting: `read -ep '$ '` looks the same. */
const PROMPT_END = /[$#%>❯➜➤λ»›→] ?$/;
/** Python's and the like: the end looks right, and the line is no shell's. */
const NOT_SHELL = /(?:^|\s)(?:>>>|\.\.\.) ?$/;
/** A prompt that asks for something secret, in the languages seen most. Also only for counting: nothing here can be sure. */
const ASKS_SECRET = /pass(?:word|phrase|code|wd)|парол|кодов|passwort|kennwort|contrase|mot de passe|\bpin\b|\botp\b|密码|パスワード/i;

type RowLook = "prompt" | "password" | "other";

export function rowLook(left: string): RowLook {
  if (ASKS_SECRET.test(left)) return "password";
  return PROMPT_END.test(left) && !NOT_SHELL.test(left) ? "prompt" : "other";
}

/** A key on its way: where it should land and what should be there then. Holds no text. */
interface Guess {
  /** The cell it changes, and where the cursor is once its answer is in. */
  col: number;
  cursor: number;
  /** Fingerprints of what the cell should hold after it, and what it holds till then. */
  want: number;
  was: number;
  erase: boolean;
  /** It was drawable when typed. */
  shown: boolean;
  /** A key that isn't typing came after it, so a later screen says nothing against it. */
  overtaken: boolean;
  place: EchoPlace;
  at: number;
}

type Verdict = "right" | "wrong" | "silent" | "unjudged";

export class ShadowEcho {
  /** An echo proved this line takes keys, and nothing has happened since to cast doubt. */
  private open = false;
  private pending: Guess[] = [];
  /** The row and column the next key would land on, while keys are on their way. */
  private row = 0;
  private next = 0;
  /** Columns the cursor can be in while they are. */
  private lo = 0;
  private hi = 0;
  /** Where the proved typing starts, and a fingerprint of the row left of it. */
  private spanStart = 0;
  private before = 0;
  /** A key that isn't typing was sent and no output has come since, so the cursor's place is old news. */
  private awaiting = false;
  private look: RowLook = "other";
  private lookRow = -1;
  private readonly salt: number;
  private readonly blank: number;

  constructor(
    private readonly screen: EchoScreen,
    private readonly sink: EchoSink,
    private readonly place: () => EchoPlace,
    salt: number = crypto.getRandomValues(new Uint32Array(1))[0],
  ) {
    this.salt = salt;
    this.blank = this.fingerprint("");
  }

  /** Something the terminal is sending, before it goes. */
  key(data: string) {
    const kind = keyKind(data);
    if (kind === "none") return;
    this.settle();
    if (kind === "break") return this.boundary(true);
    const place = this.place();
    this.sink.count(place, "keys");
    if (kind === "other" || kind === "soft") return this.boundary(kind === "other");
    this.sink.count(place, "typed");
    if (kind === "erase") this.guessErase(place);
    else this.guessChar(data.codePointAt(0) ?? 0, place);
  }

  /** Output reached the terminal's buffer. */
  parsed() {
    this.settle();
    this.awaiting = false;
  }

  /** Every so often: keys that got no answer are given up on, and their fingerprints go. */
  tick() {
    this.settle();
  }

  /** What's on screen stopped meaning what it did: a new size, the connection, the other screen. */
  reset() {
    this.drop("unjudged");
    this.boundary(true);
  }

  private guessChar(code: number, place: EchoPlace) {
    // An Enter's or a Tab's answer is on its way: nobody knows yet where this lands.
    if (this.awaiting || this.pending[0]?.overtaken) return this.sink.count(place, "waited");
    if (!this.pending.length) {
      const { row, col } = this.cursor();
      this.row = row;
      this.next = this.lo = this.hi = col;
    }
    if (this.lookRow !== this.row) {
      this.lookRow = this.row;
      this.look = rowLook(this.leftOf(this.next));
    }
    if (this.pending.length >= MOST_ON_THE_WAY || !oneCell(code) || this.next >= this.screen.cols - 1) {
      this.sink.count(place, "unfit");
      return this.boundary(false);
    }
    // Cells a Backspace on its way is about to clear don't count.
    if (this.hasTextFrom(this.hi)) {
      this.sink.count(place, "midLine");
      return this.boundary(false);
    }
    const shown = this.open;
    this.sink.count(place, shown ? "atOnce" : "waited");
    if (shown && this.look === "prompt") this.sink.count(place, "atPrompt");
    const col = this.next;
    this.next = col + 1;
    this.hi = Math.max(this.hi, this.next);
    this.pending.push({ col, cursor: col + 1, want: this.fingerprint(String.fromCodePoint(code)), was: this.blank, erase: false, shown, overtaken: false, place, at: Date.now() });
  }

  private guessErase(place: EchoPlace) {
    const last = this.pending[this.pending.length - 1];
    // Only over what was proved. Over a key still on its way, what's on screen
    // now looks just like the Backspace having arrived.
    if (!this.open || this.next <= this.spanStart || (last && !last.erase)) {
      this.sink.count(place, "waited");
      return this.boundary(false);
    }
    this.sink.count(place, "atOnce");
    if (this.look === "prompt") this.sink.count(place, "atPrompt");
    const col = this.next - 1;
    this.next = col;
    this.lo = Math.min(this.lo, col);
    this.pending.push({ col, cursor: col, want: this.blank, was: this.cellPrint(this.row, col), erase: true, shown: true, overtaken: false, place, at: Date.now() });
  }

  /** The line isn't what was being guessed any more. What's on its way can still come true, but nothing new is guessed from it. */
  private boundary(forgetRow: boolean) {
    this.open = false;
    this.awaiting = true;
    if (forgetRow) this.lookRow = -1;
    for (const g of this.pending) g.overtaken = true;
  }

  /** Compares what's on screen with what's on its way, and watches the proved line. */
  private settle() {
    const cur = this.cursor();
    if (this.pending.length) this.judge(cur);
    if (this.open) this.watch(cur);
  }

  private judge(cur: { row: number; col: number }) {
    const now = Date.now();
    if (cur.row !== this.row) {
      // The line is left behind. Its cells still say what landed before that.
      for (const g of this.pending.splice(0)) {
        const landed = g.want !== this.blank && this.cellPrint(this.row, g.col) === g.want;
        this.settled(g, landed ? "right" : g.overtaken ? "unjudged" : "wrong", cur, now);
      }
      return;
    }
    // Answers come in order: the newest key whose answer is on screen, and all before it, are in.
    let upTo = -1;
    for (let k = this.pending.length - 1; k >= 0 && upTo < 0; k--) {
      const g = this.pending[k];
      if (g.cursor === cur.col && this.cellPrint(this.row, g.col) === g.want) upTo = k;
    }
    const landed = this.pending.splice(0, upTo + 1);
    landed.forEach((g, i) => {
      const redone = landed.slice(i + 1).some((later) => later.col === g.col);
      this.settled(g, redone || this.cellPrint(this.row, g.col) === g.want ? "right" : g.overtaken ? "unjudged" : "wrong", cur, now);
    });
    while (this.pending.length) {
      const g = this.pending[0];
      const late = now - g.at >= GIVE_UP_MS;
      if (g.overtaken) {
        if (!late) return;
        this.pending.shift();
        this.settled(g, "unjudged", cur, now);
        continue;
      }
      const here = this.cellPrint(this.row, g.col);
      // Something other than what was guessed, or the cursor went past a cell left as it was.
      if (here !== g.want && (here !== g.was || (!g.erase && cur.col >= g.cursor))) {
        this.pending.shift();
        this.settled(g, "wrong", cur, now);
        this.open = false;
        this.lookRow = -1;
        return this.drop("dropped");
      }
      if (!late) return;
      this.pending.shift();
      this.settled(g, "silent", cur, now);
      // The line stopped taking keys. Later ones run out on their own time.
      this.open = false;
      this.lookRow = -1;
    }
  }

  private settled(g: Guess, verdict: Verdict, cur: { row: number; col: number }, now: number) {
    if (verdict === "right") {
      if (g.erase || g.want !== this.blank) this.sink.echoTook(g.place, now - g.at);
      if (g.shown) this.sink.count(g.place, "right");
      else this.prove(g, cur);
    } else if (g.shown) this.sink.count(g.place, verdict);
    else if (verdict === "silent") this.sink.count(g.place, "heldSilent");
  }

  /** A key that had to wait came back where it should: the line takes keys. A blank proves nothing, since it looks the same before and after. */
  private prove(g: Guess, cur: { row: number; col: number }) {
    if (this.open || g.overtaken || g.erase || g.want === this.blank || cur.row !== this.row || cur.col <= g.col) return;
    this.open = true;
    this.spanStart = g.col;
    this.before = this.rowPrint(this.row, g.col);
    if (this.look === "password") this.sink.count(g.place, "passwordOpen");
  }

  /** Shuts the gate if the cursor isn't where the keys say, or the text before them changed: something other than the line editor is writing here. */
  private watch(cur: { row: number; col: number }) {
    const onWay = this.pending.length > 0;
    const inPlace = cur.row === this.row && (onWay ? cur.col >= this.lo && cur.col <= this.hi : cur.col === this.next);
    if (!inPlace || this.rowPrint(this.row, this.spanStart) !== this.before) {
      this.open = false;
      this.lookRow = -1;
      this.drop("dropped");
    } else if (!onWay) {
      this.lo = this.hi = this.next;
    }
  }

  /** Forgets what's on its way. */
  private drop(as: "dropped" | "unjudged") {
    for (const g of this.pending.splice(0)) if (g.shown) this.sink.count(g.place, g.overtaken ? "unjudged" : as);
  }

  private cursor(): { row: number; col: number } {
    const b = this.screen.buffer.active;
    return { row: b.baseY + b.cursorY, col: b.cursorX };
  }

  private chars(row: number, col: number): string {
    return this.screen.buffer.active.getLine(row)?.getCell(col)?.getChars() ?? "";
  }

  private hasTextFrom(col: number): boolean {
    for (let x = col; x < this.screen.cols; x++) if (this.chars(this.row, x).trim()) return true;
    return false;
  }

  /** The row's text left of a column. Read for a moment to tell what kind of row it is, never kept. */
  private leftOf(col: number): string {
    let text = "";
    for (let x = 0; x < col; x++) text += this.chars(this.row, x) || " ";
    return text;
  }

  private cellPrint(row: number, col: number): number {
    return this.fingerprint(this.chars(row, col));
  }

  private rowPrint(row: number, end: number): number {
    let print = this.salt;
    for (let x = 0; x < end; x++) print = mix(print, this.chars(row, x) || " ");
    return print;
  }

  /** An empty cell and a space are the same thing to look at. */
  private fingerprint(chars: string): number {
    return mix(this.salt, chars || " ");
  }
}

/** FNV-1a. */
function mix(seed: number, text: string): number {
  let h = seed ^ 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return h >>> 0;
}
