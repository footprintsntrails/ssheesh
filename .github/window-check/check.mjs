// Runs the app on real Windows with the real mouse and keyboard. Fails if the
// window's focus keeps moving by itself, or a typed command doesn't reach the
// shell: the browser tests and Linux can't see either. The debug port is only
// used to find the first screen's button, then it's let go of.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [exe, tool, out] = process.argv.slice(2);
// Busiest second a healthy window had, in focus events: 3 at rest, 53 with Alt+Tab 16 times a second.
const MAX_AT_REST = 30;
const MAX_ALT_TAB = 150;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const win = (...a) => spawnSync(tool, a.map(String), { encoding: "utf8" });
mkdirSync(out, { recursive: true });
const marks = join(out, "marks");
mkdirSync(marks, { recursive: true });

// One server, so the first screen is the same each time. Nothing answers at this address.
const data = join(process.env.APPDATA, "io.github.footprintsntrails.ssheesh");
mkdirSync(data, { recursive: true });
writeFileSync(join(data, "hosts.json"), JSON.stringify([{ id: "check", name: "check", hostname: "203.0.113.10", user: "check", port: 22, identityFile: null, source: "saved", rememberedPassword: false, identityAgent: null }]));

const app = spawn(exe, [], { stdio: "ignore" });
let target;
for (let i = 0; i < 90 && !target; i++) {
  await sleep(1000);
  try {
    target = (await (await fetch("http://127.0.0.1:9222/json")).json()).find((t) => t.type === "page");
  } catch {}
}
if (!target) {
  win("shot", join(out, "no-window.png"));
  console.log("Can't tell: the app's page never came up");
  process.exit(2);
}
const [ox, oy] = win("origin", app.pid).stdout.trim().split(" ").map(Number);

// No Playwright: it fakes focus, which is what this checks.
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let asked = 0;
const answers = new Map();
ws.onmessage = (e) => {
  const m = JSON.parse(e.data);
  answers.get(m.id)?.(m.result?.result?.value);
  answers.delete(m.id);
};
const ask = (expression) =>
  new Promise((res) => {
    const id = ++asked;
    answers.set(id, res);
    ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, returnByValue: true } }));
    setTimeout(() => res(undefined), 5000);
  });
const SKIP = `[...document.querySelectorAll("button")].find((b) => b.textContent.includes("Skip setup"))`;

const stop = join(out, "stop");
const file = join(out, "watch.jsonl");
spawn(tool, ["watch", app.pid, file, stop], { stdio: "ignore" });

const problems = [];
let n = 0;
async function typed() {
  const name = `m${++n}.txt`;
  win("keys", `echo x > ${join(marks, name)}{n}`);
  for (let i = 0; i < 30 && !existsSync(join(marks, name)); i++) await sleep(500);
  return existsSync(join(marks, name));
}
async function phase(name, limit, run) {
  const from = Date.now();
  await run();
  const to = Date.now();
  await sleep(1100);
  const lines = readFileSync(file, "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l)).filter((l) => l.at >= from && l.at <= to + 1000);
  const busiest = lines.reduce((s, l) => Math.max(s, l.focus), 0);
  console.log(`${name}: busiest second ${busiest} focus events, window answered within ${lines.reduce((s, l) => Math.max(s, l.pingMax), 0)} ms`);
  if (busiest > limit) problems.push(`${name}: ${busiest} focus events in a second, over ${limit}`);
  win("shot", join(out, `${name}.png`));
}

await phase("start", MAX_AT_REST, () => sleep(10000));
await phase("skip-welcome", MAX_AT_REST, async () => {
  for (let i = 0; i < 5; i++) {
    const at = await ask(`(() => { const r = ${SKIP}?.getBoundingClientRect(); return r ? [Math.round(r.x + r.width / 2), Math.round(r.y + r.height / 2)] : null; })()`);
    if (!Array.isArray(at)) return;
    win("click", ox + at[0], oy + at[1]);
    await sleep(1500);
  }
  problems.push("a click on the first screen's Skip setup did nothing, 5 times");
});
await phase("new-terminal", MAX_AT_REST, async () => {
  win("newterm");
  for (let i = 0; i < 40 && !(await ask(`!!document.querySelector(".xterm")`)); i++) await sleep(500);
  if (!(await ask(`!!document.querySelector(".xterm")`))) problems.push("Ctrl+Shift+T opened no terminal");
  // A new shell takes its time to start on a runner.
  await sleep(4000);
  win("click", ox + 550, oy + 400);
  await sleep(500);
  ws.close();
});
await phase("typing", MAX_AT_REST, async () => {
  if (!(await typed())) problems.push("a typed command never reached the shell");
});
await phase("idle", MAX_AT_REST, () => sleep(10000));
const other = spawn("notepad.exe", [], { stdio: "ignore" });
await sleep(1500);
await phase("alt-tab", MAX_ALT_TAB, async () => {
  // Notepad has the keys now, so this first one brings the app back.
  win("alttab");
  await sleep(800);
  let reached = 0;
  for (let i = 0; i < 10; i++) {
    if (await typed()) reached++;
    win("alttab");
    await sleep(400);
    win("alttab");
    await sleep(400);
  }
  console.log(`after Alt+Tab, ${reached} of 10 typed commands reached the shell`);
  if (reached < 10) problems.push(`after Alt+Tab only ${reached} of 10 typed commands reached the shell`);
});

writeFileSync(stop, "");
await sleep(300);
other.kill();
spawnSync("taskkill", ["/F", "/T", "/PID", String(app.pid)]);
if (problems.length) {
  console.log(`FAIL:\n${problems.join("\n")}`);
  process.exit(1);
}
console.log("OK: focus stays put, keys arrive");
process.exit(0);
