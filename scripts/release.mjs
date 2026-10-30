// Usage: npm run release 0.0.2
// Sets the version in every manifest, commits, and tags vX.Y.Z.
// Push with `git push --follow-tags` to start the release build.

import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version ?? "")) {
  console.error("Usage: npm run release <version>, for example: npm run release 0.0.2");
  process.exit(1);
}

const run = (cmd) => execSync(cmd, { stdio: "inherit" });

if (execSync("git status --porcelain").toString().trim()) {
  console.error("Commit or stash your changes first.");
  process.exit(1);
}

const json = (path, set) => {
  const data = JSON.parse(readFileSync(path, "utf8"));
  set(data);
  writeFileSync(path, JSON.stringify(data, null, 2) + "\n");
};
const toml = (path) => {
  const text = readFileSync(path, "utf8").replace(/^version = ".*"$/m, `version = "${version}"`);
  writeFileSync(path, text);
};

json("package.json", (d) => (d.version = version));
json("package-lock.json", (d) => {
  d.version = version;
  if (d.packages?.[""]) d.packages[""].version = version;
});
json("src-tauri/tauri.conf.json", (d) => (d.version = version));
toml("src-tauri/Cargo.toml");
toml("crates/sshee-core/Cargo.toml");
run("cargo update --workspace --offline");

run("git add -A");
run(`git commit -m "Release v${version}"`);
run(`git tag v${version}`);
console.log(`\nTagged v${version}. Now run:\n\n  git push --follow-tags\n`);
