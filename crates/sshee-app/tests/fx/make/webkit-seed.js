// Made-up records for the real 0.10.14 under WebKitGTK, run in its page through wk-eval.mjs;
// it keeps the settings and labels the app had written and reloads, so the app reads these back itself.
(() => {
  const put = (k, state, version) => localStorage.setItem(k, JSON.stringify({ state, version }));
  const s = JSON.parse(localStorage.getItem("sshee-settings"));
  Object.assign(s.state, {
    updateMode: "manual",
    streamerMode: true,
    terminalFontSize: 14,
    welcomeNote: "Сервер базы — только чтение. Café ☕",
    highlightRules: [{ id: "hr-1", pattern: "FATAL", color: "#e5534b" }],
  });
  localStorage.setItem("sshee-settings", JSON.stringify(s));
  const l = JSON.parse(localStorage.getItem("sshee-labels"));
  Object.assign(l.state, {
    env: { "cfg:db-main": "prod", "cfg:web-7731": "staging", "saved:k3v9x2qa": "dev" },
    looks: { "cfg:db-main": { icon: "database", color: "#e5534b" } },
    autoLooks: { "cfg:web-7731": { icon: "globe" } },
  });
  localStorage.setItem("sshee-labels", JSON.stringify(l));
  put("sshee-snippets", { snippets: [
    { id: "sn-r1", name: "Restart web", command: "sudo systemctl restart nginx" },
    { id: "sn-t2", name: "Токен деплоя", command: "export DEPLOY_TOKEN=tok_made_up_4f7a1c9e2b" },
  ] }, 1);
  put("sshee-bookmarks", {
    pins: { "host:cfg:web-7731": ["/var/www/app", "/etc/nginx"] },
    recent: { "host:cfg:db-main": ["/var/lib/postgresql/16/main"] },
  }, 1);
  put("sshee-tunnels", { byHost: { "cfg:db-main": [
    { id: "tn-pg", name: "Postgres", forward: { kind: "local", localPort: 15432, remoteHost: "127.0.0.1", remotePort: 5432 }, on: false },
    { id: "tn-sx", forward: { kind: "socks", localPort: 1080 }, on: false },
  ] } }, 1);
  put("sshee-typing-counts", { saved: {} }, 0);
  put("sshee-workspace", {
    layouts: {
      "host:cfg:web-7731": { kind: "split", id: "sp-w1", dir: "row", children: [{ kind: "pane", id: "pn-w1" }, { kind: "pane", id: "pn-w2" }] },
      "host:cfg:db-main": { kind: "pane", id: "pn-d1" },
    },
    splitOn: { "host:cfg:web-7731": true },
    panes: {
      "pn-w1": { id: "pn-w1", kind: "terminal", target: { kind: "host", hostId: "cfg:web-7731" }, sessionKey: "q8x2m4k7za", n: 1 },
      "pn-w2": { id: "pn-w2", kind: "files", target: { kind: "host", hostId: "cfg:web-7731" }, path: "/var/www/app", n: 1 },
      "pn-d1": { id: "pn-d1", kind: "terminal", target: { kind: "host", hostId: "cfg:db-main" }, sessionKey: "t5n1b9w3ce", n: 1 },
    },
    focused: "pn-w1", hidden: [], tabOrder: ["pn-w1", "pn-w2", "pn-d1"], home: false,
    lastFocusedFor: { "host:cfg:web-7731": "pn-w1", "host:cfg:db-main": "pn-d1" }, frontOf: {}, closedTabs: [],
  }, 2);
  setTimeout(() => location.reload(), 300);
  return Object.keys(localStorage).length;
})()
