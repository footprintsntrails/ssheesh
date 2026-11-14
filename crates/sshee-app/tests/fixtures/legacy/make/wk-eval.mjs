// Evaluates one JS expression in the app's page through WebKitGTK's remote inspector.
// Usage: node wk-eval.mjs <file-with-expression>
import { readFileSync } from "node:fs";
const expr = readFileSync(process.argv[2], "utf8");
const ws = new WebSocket("ws://127.0.0.1:9339/socket/1/1/WebPage");
let target = null;
const done = (code) => { try { ws.close(); } catch {} ; process.exit(code); };
setTimeout(() => { console.error("timeout"); done(2); }, 15000);
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.method === "Target.targetCreated" && !target) {
    target = msg.params.targetInfo.targetId;
    const inner = { id: 1, method: "Runtime.evaluate", params: { expression: expr, returnByValue: true, awaitPromise: true } };
    ws.send(JSON.stringify({ id: 100, method: "Target.sendMessageToTarget", params: { targetId: target, message: JSON.stringify(inner) } }));
  } else if (msg.method === "Target.dispatchMessageFromTarget") {
    const inner = JSON.parse(msg.params.message);
    if (inner.id === 1) {
      console.log(JSON.stringify(inner.result ?? inner.error, null, 1));
      done(0);
    }
  }
};
ws.onerror = (e) => { console.error("ws error", e.message); done(1); };
