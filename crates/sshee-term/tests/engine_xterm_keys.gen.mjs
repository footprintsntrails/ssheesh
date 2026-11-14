// Writes engine_xterm_keys.json: the bytes xterm.js 6.0 (the terminal in SSHeesh 0.10) sends
// for a key, so `sshee-term`'s port (src/input.rs) can be checked against it byte for byte.
//
// It runs xterm.js's own keyboard code from node_modules (common/input/Keyboard.ts, compiled
// here with the project's TypeScript), plus the few lines of its keydown and keypress handlers
// (browser/CoreBrowserTerminal.ts, `_keyDown`, `_keyPress`, `_isThirdLevelShift`) that decide
// whether that result or the typed character goes out. Those lines are copied below with the
// parts that need a browser (composition, screen reader, scrolling) left out.
//
// xterm.js is MIT licensed: Copyright (c) 2017-2019, The xterm.js authors; Copyright (c)
// 2014-2016, SourceLair Private Company; Copyright (c) 2012-2013, Christopher Jeffrey.
//
// Run from the repository root:  node crates/sshee-term/tests/engine_xterm_keys.gen.mjs
//
// Each row: [key, code, mods, platform, appCursor, sent, action, text]
//   key, code  as a browser's KeyboardEvent has them ("a", "KeyA"; "ArrowUp", "ArrowUp")
//   mods       1 Shift, 2 Alt, 4 Ctrl, 8 Meta (⌘ / Win), 16 AltGraph held (Windows)
//   platform   "l" Linux, "w" Windows, "m" macOS with Option as Meta (0.10's setting),
//              "M" macOS with Option typing characters
//   appCursor  0 or 1 (`ESC [ ? 1 h`)
//   sent       what goes to the program ("" for nothing)
//   action     "" or "pageUp", "pageDown", "selectAll" (xterm.js scrolled or selected instead)
//   text       the character the system typed for the press, or null when it typed none. A
//              browser sends keypress only then. Ctrl+Alt on Windows types something only
//              where the layout has an AltGr character there (the AltGr rows below give it);
//              Ctrl+Alt+C on a US or Russian layout types nothing, so nothing went out.
//
// keyCode, which xterm.js reads, isn't in the row: it is derived from key and code the same
// way src/input.rs derives it (`legacy_key_code`), as Chrome on Windows fills it in: a Latin
// letter's own code, else the US key at that place.

import fs from "node:fs";
import path from "node:path";
import url from "node:url";
import { createRequire } from "node:module";

const here = path.dirname(url.fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const require = createRequire(path.join(root, "package.json"));
const ts = require("typescript");
const xtermSrc = path.join(root, "node_modules/@xterm/xterm/src");
const xtermVersion = JSON.parse(fs.readFileSync(path.join(root, "node_modules/@xterm/xterm/package.json"), "utf8")).version;

/** Compiles one of xterm.js's own source files and runs it as a CommonJS module. */
function load(file, deps) {
  const source = fs.readFileSync(path.join(xtermSrc, file), "utf8");
  const js = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, preserveConstEnums: true },
  }).outputText;
  const module = { exports: {} };
  new Function("require", "module", "exports", js)((name) => deps[name] ?? {}, module, module.exports);
  return module.exports;
}

const Types = load("common/Types.ts", {});
const EscapeSequences = load("common/data/EscapeSequences.ts", {});
const { evaluateKeyboardEvent } = load("common/input/Keyboard.ts", {
  "common/Types": Types,
  "common/data/EscapeSequences": EscapeSequences,
});
const { KeyboardResultType } = Types;
if (!KeyboardResultType || KeyboardResultType.PAGE_UP === undefined) throw new Error("KeyboardResultType not found");

// ---- keyCode, as src/input.rs derives it

const NAMED = {
  Backspace: 8, Tab: 9, Enter: 13, Shift: 16, Control: 17, Alt: 18, Pause: 19, CapsLock: 20, Escape: 27,
  PageUp: 33, PageDown: 34, End: 35, Home: 36, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40,
  Insert: 45, Delete: 46, Meta: 91, ContextMenu: 93, AltGraph: 225,
};
for (let n = 1; n <= 24; n++) NAMED[`F${n}`] = 111 + n;
const PLACE = {
  Semicolon: 186, Equal: 187, Comma: 188, Minus: 189, Period: 190, Slash: 191, Backquote: 192,
  BracketLeft: 219, Backslash: 220, BracketRight: 221, Quote: 222, Space: 32,
  NumpadMultiply: 106, NumpadAdd: 107, NumpadSubtract: 109, NumpadDecimal: 110, NumpadDivide: 111,
};

function keyCodeFor(key, code) {
  if (NAMED[key] !== undefined) return NAMED[key];
  if (key.length === 1 && /[a-zA-Z]/.test(key)) return key.toUpperCase().charCodeAt(0);
  let m;
  if ((m = /^Key([A-Z])$/.exec(code))) return m[1].charCodeAt(0);
  if ((m = /^Digit(\d)$/.exec(code))) return 48 + Number(m[1]);
  if ((m = /^Numpad(\d)$/.exec(code))) return 96 + Number(m[1]);
  if (PLACE[code] !== undefined) return PLACE[code];
  if (key.length === 1 && /\d/.test(key)) return 48 + Number(key);
  return 0;
}

// ---- xterm.js's keydown and keypress, for one key on a fresh terminal

const PLATFORMS = {
  l: { isMac: false, isWindows: false, macOptionIsMeta: true },
  w: { isMac: false, isWindows: true, macOptionIsMeta: true },
  m: { isMac: true, isWindows: false, macOptionIsMeta: true },
  M: { isMac: true, isWindows: false, macOptionIsMeta: false },
};

function isThirdLevelShift(p, ev) {
  const thirdLevelKey =
    (p.isMac && !p.macOptionIsMeta && ev.altKey && !ev.ctrlKey && !ev.metaKey) ||
    (p.isWindows && ev.altKey && ev.ctrlKey && !ev.metaKey) ||
    (p.isWindows && ev.getModifierState("AltGraph"));
  if (ev.type === "keypress") return thirdLevelKey;
  return thirdLevelKey && (!ev.keyCode || ev.keyCode > 47);
}

/** What the system types for a key with these modifiers when nobody says otherwise. */
function typedText(key, mods, platform) {
  if (key.length !== 1) return null;
  if (platform === "w" && mods & 2 && mods & 4) return null;
  return key;
}

function press(key, code, mods, platform, appCursor, text) {
  const p = PLATFORMS[platform];
  const ev = {
    key,
    code,
    keyCode: keyCodeFor(key, code),
    shiftKey: !!(mods & 1),
    altKey: !!(mods & 2),
    ctrlKey: !!(mods & 4),
    metaKey: !!(mods & 8),
    type: "keydown",
    getModifierState: (m) => m === "AltGraph" && !!(mods & 16),
  };
  // _keyDown
  let unprocessedDeadKey = false;
  const shouldIgnoreComposition = p.isMac && p.macOptionIsMeta && ev.altKey;
  if (!shouldIgnoreComposition && (ev.key === "Dead" || ev.key === "AltGraph")) unprocessedDeadKey = true;
  const result = evaluateKeyboardEvent(ev, !!appCursor, p.isMac, p.macOptionIsMeta);
  if (result.type === KeyboardResultType.PAGE_DOWN) return ["", "pageDown"];
  if (result.type === KeyboardResultType.PAGE_UP) return ["", "pageUp"];
  const action = result.type === KeyboardResultType.SELECT_ALL ? "selectAll" : "";
  let toKeyPress = false;
  if (isThirdLevelShift(p, ev)) toKeyPress = true;
  else if (!result.key) toKeyPress = true;
  else if (ev.key && !ev.ctrlKey && !ev.altKey && !ev.metaKey && ev.key.length === 1 && ev.key.charCodeAt(0) >= 65 && ev.key.charCodeAt(0) <= 90) toKeyPress = true;
  else if (unprocessedDeadKey) return ["", action];
  else return [result.key, action];
  if (!toKeyPress) return ["", action];
  // _keyPress: a browser sends one only when the system typed a character (its charCode).
  if (ev.key.length !== 1 || text === null) return ["", action];
  const kp = { ...ev, type: "keypress" };
  if ((kp.altKey || kp.ctrlKey || kp.metaKey) && !isThirdLevelShift(p, kp)) return ["", action];
  return [String.fromCharCode(text.charCodeAt(0)), action];
}

// ---- the keys

const rows = [];
const seen = new Set();
function add(key, code, mods, platforms, appCursors = [0], text) {
  for (const platform of platforms)
    for (const appCursor of appCursors) {
      const id = JSON.stringify([key, code, mods, platform, appCursor]);
      if (seen.has(id)) continue;
      seen.add(id);
      const typed = text === undefined ? typedText(key, mods, platform) : text;
      rows.push([key, code, mods, platform, appCursor, ...press(key, code, mods, platform, appCursor, typed), typed]);
    }
}

const ALL_MODS = [...Array(16).keys()];
const PCS = ["l", "w", "m"];

// US layout: every key with every mix of Shift, Alt, Ctrl and Meta. Shift changes what it types.
const us = [];
for (const c of "abcdefghijklmnopqrstuvwxyz") us.push([c, c.toUpperCase(), `Key${c.toUpperCase()}`]);
const shifted = ")!@#$%^&*(";
for (let d = 0; d <= 9; d++) us.push([String(d), shifted[d], `Digit${d}`]);
for (const [code, plain, shift] of [
  ["Semicolon", ";", ":"], ["Equal", "=", "+"], ["Comma", ",", "<"], ["Minus", "-", "_"], ["Period", ".", ">"],
  ["Slash", "/", "?"], ["Backquote", "`", "~"], ["BracketLeft", "[", "{"], ["Backslash", "\\", "|"],
  ["BracketRight", "]", "}"], ["Quote", "'", '"'], ["Space", " ", " "],
]) us.push([plain, shift, code]);
for (const [plain, shift, code] of us) for (const mods of ALL_MODS) add(mods & 1 ? shift : plain, code, mods, PCS);

// Named keys: also in application cursor mode.
const named = ["Backspace", "Tab", "Enter", "Escape", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Insert", "Delete", "Home", "End", "PageUp", "PageDown"];
for (let n = 1; n <= 13; n++) named.push(`F${n}`);
for (const k of named) for (const mods of ALL_MODS) add(k, k, mods, PCS, [0, 1]);
// Modifiers pressed on their own send nothing.
for (const [k, code] of [["Shift", "ShiftLeft"], ["Control", "ControlLeft"], ["Alt", "AltLeft"], ["Meta", "MetaLeft"], ["CapsLock", "CapsLock"], ["AltGraph", "AltRight"]])
  for (const mods of [0, 1, 2, 4, 8]) add(k, code, mods, PCS);

// The keypad, with Num Lock on.
for (let d = 0; d <= 9; d++) for (const mods of [0, 1, 2, 4]) add(String(d), `Numpad${d}`, mods, PCS);
for (const [k, code] of [["+", "NumpadAdd"], ["-", "NumpadSubtract"], ["*", "NumpadMultiply"], ["/", "NumpadDivide"], [".", "NumpadDecimal"], ["Enter", "NumpadEnter"]])
  for (const mods of [0, 1, 2, 4]) add(k, code, mods, PCS, [0, 1]);

// Other layouts: plain, Shift, Ctrl, Alt, Ctrl+Alt, Ctrl+Shift, Alt+Shift.
const LAYOUT_MODS = [0, 1, 2, 4, 6, 5, 3];
const russian = [
  ["ф", "Ф", "KeyA"], ["и", "И", "KeyB"], ["с", "С", "KeyC"], ["в", "В", "KeyD"], ["у", "У", "KeyE"], ["а", "А", "KeyF"],
  ["м", "М", "KeyV"], ["ч", "Ч", "KeyX"], ["я", "Я", "KeyZ"], ["й", "Й", "KeyQ"], ["ц", "Ц", "KeyW"], ["ш", "Ш", "KeyI"],
  ["х", "Х", "BracketLeft"], ["ъ", "Ъ", "BracketRight"], ["ж", "Ж", "Semicolon"], ["э", "Э", "Quote"], ["б", "Б", "Comma"],
  ["ю", "Ю", "Period"], [".", ",", "Slash"], ["ё", "Ё", "Backquote"], ["1", "!", "Digit1"], ["2", '"', "Digit2"], ["3", "№", "Digit3"],
];
const german = [
  ["z", "Z", "KeyY"], ["y", "Y", "KeyZ"], ["ü", "Ü", "BracketLeft"], ["ö", "Ö", "Semicolon"], ["ä", "Ä", "Quote"],
  ["ß", "?", "Minus"], ["#", "'", "Backslash"], ["+", "*", "BracketRight"], ["<", ">", "IntlBackslash"], ["^", "°", "Backquote"],
];
const french = [["a", "A", "KeyQ"], ["q", "Q", "KeyA"], ["w", "W", "KeyZ"], ["&", "1", "Digit1"], ["é", "2", "Digit2"], ["ù", "%", "Quote"]];
for (const [plain, shift, code] of [...russian, ...german, ...french])
  for (const mods of LAYOUT_MODS) add(mods & 1 ? shift : plain, code, mods, PCS);

// AltGr: on Windows it is Ctrl+Alt (and the AltGraph state), and the layout types the character
// (German here); on Linux the character arrives on its own.
for (const [k, code] of [["@", "KeyQ"], ["€", "KeyE"], ["{", "Digit7"], ["[", "Digit8"], ["]", "Digit9"], ["}", "Digit0"], ["\\", "Minus"], ["~", "BracketRight"], ["|", "IntlBackslash"]]) {
  add(k, code, 2 | 4 | 16, ["w"], [0], k);
  add(k, code, 2 | 4, ["w"], [0], k);
  add(k, code, 2 | 4, ["l"]);
  add(k, code, 0, ["l", "w"]);
}

// macOS Option: with Option as Meta (0.10) it sends Esc and the key; without, the character.
for (const [k, code] of [["å", "KeyA"], ["∫", "KeyB"], ["ç", "KeyC"], ["∑", "KeyW"], ["¡", "Digit1"], ["™", "Digit2"], ["“", "BracketLeft"], ["…", "Semicolon"], ["Dead", "KeyE"], ["Dead", "KeyN"], ["Dead", "KeyU"], ["Dead", "Backquote"]])
  for (const mods of [2, 3, 6]) add(k, code, mods, ["m", "M"]);
for (const k of ["ArrowLeft", "ArrowRight", "Backspace", "Enter", "Escape"]) for (const mods of [2, 3, 8, 10]) add(k, k, mods, ["M"], [0, 1]);
// Dead keys elsewhere: xterm.js sends nothing for them (the composed character comes as text).
for (const mods of [0, 1, 2]) add("Dead", "BracketLeft", mods, ["l", "w"]);

const out = {
  about: `What xterm.js ${xtermVersion} sends for a key; written by engine_xterm_keys.gen.mjs. Rows: [key, code, mods, platform, appCursor, sent, action, text].`,
  rows,
};
const file = path.join(here, "engine_xterm_keys.json");
fs.writeFileSync(file, JSON.stringify(out).replace(/\],\[/g, "],\n[") + "\n");
console.log(`${rows.length} keys written to ${path.relative(root, file)}`);
