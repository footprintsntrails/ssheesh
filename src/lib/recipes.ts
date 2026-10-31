// Step-by-step guides for common server jobs. Every step says in plain words
// what it does and why, then gives one command to paste into a terminal and
// run yourself. Nothing here runs anything.
//
// Commands are single lines (a newline would run them on paste), aimed at
// Debian 12/13, Ubuntu 22.04/24.04 and Fedora/RHEL 9, and safe to run twice.

import type { Host } from "./types";

/** Debian and Ubuntu (apt), or Fedora, RHEL and its rebuilds (dnf). */
export type Os = "debian" | "rhel";

export const OS_LABEL: Record<Os, string> = { debian: "Debian / Ubuntu", rhel: "Fedora / RHEL" };

export interface SshOptions {
  /** Log in as this user instead of the usual one. */
  user?: string;
  /** Use keys only, never a password. */
  keyOnly?: boolean;
  /** A key file to offer, like "~/.ssh/id_ed25519". */
  identity?: string;
  /** A command to run on the server instead of opening a shell. */
  run?: string;
  /** Give `run` a terminal, so sudo can ask for a password. */
  tty?: boolean;
}

/** What a step's text and command may depend on. */
export interface RecipeContext {
  os: Os;
  /** The port SSHeesh connects to. */
  hostPort: number;
  /** The SSH port to use in commands: the one typed in, or `hostPort`. */
  sshPort: number;
  /** The recipe's fields that are filled in and valid, by id. */
  values: Record<string, string>;
  /** An ssh command, run on your computer, that logs in to this server afresh. */
  ssh(o?: SshOptions): string;
}

export type Text = string | ((c: RecipeContext) => string);
export type PerOs<T> = { debian: T; rhel: T };
export type Value = Text | PerOs<Text>;

export interface Field {
  id: string;
  label: string;
  /** "key" picks a public key from this computer, or takes one pasted in. */
  kind: "text" | "choice" | "key";
  help?: string;
  placeholder?: string;
  pattern?: RegExp;
  /** Shown when the text doesn't match `pattern`. */
  invalid?: string;
  choices?: { value: string; label: string }[];
  initial?: Text;
}

export interface Step {
  /** Plain text, unique within its recipe. */
  title: string;
  /** One or two sentences: what it does and why. */
  explain: Value;
  command: Value;
  /** Where to run it: on the server (the default), or on your own computer. */
  where?: "server" | "local";
  /** Only for this kind of system. */
  only?: Os;
  /** Said before the command: what could go wrong. */
  warn?: Value;
  /** Can cut you off from the server; a check that you can still log in follows. */
  lockout?: boolean;
  /** Checks that the steps before it worked. */
  check?: boolean;
  /** What you should see when it worked. */
  expect?: Value;
  /** What to do when it didn't. */
  ifFails?: { explain: Value; command: Value };
  /** Fields the command uses; it's hidden until they're filled in. */
  needs?: string[];
  /** Not needed when this computer already has a key. */
  skipIfKey?: boolean;
}

export interface Recipe {
  id: string;
  group: "secure" | "setup";
  title: string;
  summary: string;
  /** Who it's for, in one line. */
  who: string;
  fields?: Field[];
  steps: Step[];
}

export const GROUP_LABEL: Record<Recipe["group"], string> = { secure: "Lock it down", setup: "Set things up" };

// ---------------------------------------------------------------- helpers

const PORT = /^([1-9]\d{0,3}|[1-5]\d{4}|6[0-4]\d{3}|65[0-4]\d{2}|655[0-2]\d|6553[0-5])$/;

export function isPort(s: string | undefined): boolean {
  return !!s && PORT.test(s.trim());
}

const SSHD_DIR = "/etc/ssh/sshd_config.d";
const RELOAD_SSH: PerOs<string> = { debian: "sudo systemctl reload ssh", rhel: "sudo systemctl reload sshd" };

/**
 * Writes sshd settings to a file of their own. sshd keeps the first value it
 * reads and reads sshd_config.d in name order, so "00-" beats cloud-init's
 * 50-cloud-init.conf. The file names match the security check-up's fixes.
 */
function sshdSet(file: string, lines: string[]): PerOs<string> {
  const write = `printf '${lines.map((l) => `${l}\\n`).join("")}' | sudo tee ${SSHD_DIR}/${file} >/dev/null && sudo sshd -t`;
  return { debian: `${write} && ${RELOAD_SSH.debian}`, rhel: `${write} && ${RELOAD_SSH.rhel}` };
}

function sshdUndo(file: string): PerOs<string> {
  const rm = `sudo rm -f ${SSHD_DIR}/${file}`;
  return { debian: `${rm} && ${RELOAD_SSH.debian}`, rhel: `${rm} && ${RELOAD_SSH.rhel}` };
}

const KEY_TYPES = new Set([
  "ssh-ed25519",
  "ssh-rsa",
  "ecdsa-sha2-nistp256",
  "ecdsa-sha2-nistp384",
  "ecdsa-sha2-nistp521",
  "sk-ssh-ed25519@openssh.com",
  "sk-ecdsa-sha2-nistp256@openssh.com",
]);

/**
 * A public key as one authorized_keys line, or null when `text` isn't one.
 * `label` (the key's file name) replaces the key's own comment, so a key read
 * from this computer carries no user or computer name. What's left of the
 * comment never needs quoting.
 */
export function parsePublicKey(text: string, label?: string): string | null {
  const line = text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .find((l) => l && !l.startsWith("#"));
  if (!line) return null;
  const [type, blob, ...comment] = line.split(/\s+/);
  if (!KEY_TYPES.has(type) || !blob || blob.length < 40 || !/^[A-Za-z0-9+/]+={0,3}$/.test(blob)) return null;
  const tag = (label ?? comment.join("-")).replace(/[^A-Za-z0-9._@-]/g, "");
  return tag ? `${type} ${blob} ${tag}` : `${type} ${blob}`;
}

const quoteArg = (s: string) => (/^[\w~./:\\-]+$/.test(s) ? s : `"${s.replace(/"/g, "")}"`);

/**
 * An ssh command that logs in to `host` from your computer, the way SSHeesh
 * does, but as a fresh login: never through a connection that's already
 * signed in (SSHeesh shares one on macOS and Linux), so it really tests
 * whether logging in works.
 */
export function sshCommand(host: Host | null, o: SshOptions = {}, windows = false): string {
  const args = ["ssh"];
  if (o.tty) args.push("-t");
  if (!windows) args.push("-o ControlPath=none");
  if (o.keyOnly) args.push("-o PreferredAuthentications=publickey");
  if (!host) {
    args.push(o.user ? `${o.user}@localhost` : "localhost");
  } else if (host.source === "saved") {
    if (host.port && host.port !== 22) args.push(`-p ${host.port}`);
    const identity = o.identity ?? host.identityFile;
    if (identity) args.push(`-i ${quoteArg(identity)}`);
    const user = o.user ?? host.user;
    args.push(user ? `${user}@${host.hostname}` : host.hostname);
  } else {
    // An entry in ~/.ssh/config: ssh reads the rest from there.
    if (o.identity) args.push(`-i ${quoteArg(o.identity)}`);
    if (o.user) args.push(`-l ${o.user}`);
    args.push(host.name);
  }
  if (o.run) args.push(o.run);
  return args.join(" ");
}

export function makeContext({ os, host, windows, values }: { os: Os; host: Host | null; windows: boolean; values: Record<string, string> }): RecipeContext {
  const hostPort = host?.port ?? 22;
  return {
    os,
    hostPort,
    sshPort: isPort(values.sshPort) ? Number(values.sshPort) : hostPort,
    values,
    ssh: (o) => sshCommand(host, o, windows),
  };
}

export function resolve(v: Value | undefined, c: RecipeContext): string {
  if (v === undefined) return "";
  const t = typeof v === "object" ? v[c.os] : v;
  return typeof t === "function" ? t(c) : t;
}

export function stepsFor(recipe: Recipe, os: Os): Step[] {
  return recipe.steps.filter((s) => !s.only || s.only === os);
}

/** Why a field's text isn't usable, or null when it is (or is still empty). */
export function fieldError(f: Field, raw: string | undefined): string | null {
  const v = raw?.trim() ?? "";
  if (!v) return null;
  if (f.kind === "key") return parsePublicKey(v) ? null : "That isn't a public key. One starts with ssh-ed25519, ssh-rsa or ecdsa-sha2-.";
  if (f.kind === "choice") return f.choices?.some((c) => c.value === v) ? null : "Pick one of the choices.";
  return !f.pattern || f.pattern.test(v) ? null : (f.invalid ?? "That doesn't look right.");
}

/**
 * The fields that are filled in and valid. `raw` holds what was typed, by
 * field id; a field nobody touched takes its initial value. `keyFile`, the
 * picked key's path, comes along with the key.
 */
export function fieldValues(recipe: Recipe, raw: Record<string, string>, base: RecipeContext): Record<string, string> {
  const out: Record<string, string> = {};
  for (const f of recipe.fields ?? []) {
    const v = (raw[f.id] ?? resolve(f.initial, base)).trim();
    if (v && !fieldError(f, v)) out[f.id] = f.kind === "key" ? (parsePublicKey(v) ?? v) : v;
  }
  if (out.publicKey && raw.keyFile) out.keyFile = raw.keyFile;
  return out;
}

/** The fields a step still waits for. */
export function missingFields(recipe: Recipe, step: Step, values: Record<string, string>): Field[] {
  return (step.needs ?? []).filter((id) => !values[id]).map((id) => recipe.fields?.find((f) => f.id === id) as Field);
}

/**
 * Debian/Ubuntu or Fedora/RHEL, guessed from the kernel's release
 * ("Linux 6.8.0-45-generic", "5.14.0-427.el9.x86_64"); null when it doesn't
 * say. Containers report their host's kernel, so it's only a guess.
 */
export function guessOs(kernel: string): Os | null {
  const k = kernel.toLowerCase();
  if (/\.(el\d+|fc\d+|amzn\d+)/.test(k)) return "rhel";
  if (/-generic|-amd64|-arm64|-cloud|-aws|-azure|-gcp|-gke|-oracle|-kvm|-lowlatency|-virtual|-raspi|\+deb|\+rpt|ubuntu|debian/.test(k)) return "debian";
  return null;
}

// ---------------------------------------------------------------- shared fields and steps

const SSH_PORT: Field = {
  id: "sshPort",
  label: "SSH port",
  kind: "text",
  help: "SSHeesh filled in the port it connects to. The first step shows the server's own; change this if they differ.",
  placeholder: "22",
  pattern: PORT,
  invalid: "A number from 1 to 65535.",
  initial: (c) => String(c.hostPort),
};

const SEE_SSH_PORT: Step = {
  title: "See which port SSH uses",
  explain: (c) =>
    `SSHeesh reaches this server on port ${c.hostPort}. This shows the port the SSH server itself listens on. They're nearly always the same; if not, put the one shown here in the SSH port box above.`,
  command: "sudo sshd -T | grep -i '^port '",
  expect: (c) => `port ${c.sshPort}`,
};

const keyLogin = (c: RecipeContext) => c.ssh({ keyOnly: true, identity: c.values.keyFile, run: "echo Key login works" });

/** Adds the key to ~/.ssh/authorized_keys unless it's there, like ssh-copy-id. */
const addKey = (c: RecipeContext) => {
  const key = c.values.publicKey;
  const blob = key.split(" ")[1];
  const add = `mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && { grep -qF '${blob}' ~/.ssh/authorized_keys || echo '${key}' >> ~/.ssh/authorized_keys; }`;
  // SELinux only lets sshd read files labelled as SSH files.
  return c.os === "rhel" ? `${add} && restorecon -R ~/.ssh` : add;
};

/** While firewalld is off, firewall-offline-cmd edits the rules it starts with. */
const firewalldAllowSsh = (c: RecipeContext) => {
  const rule = c.sshPort === 22 ? "--add-service=ssh" : `--add-port=${c.sshPort}/tcp`;
  return `if systemctl is-active --quiet firewalld; then sudo firewall-cmd --permanent ${rule} && sudo firewall-cmd --reload; else sudo firewall-offline-cmd ${rule}; fi`;
};

const WEB_IN: PerOs<string> = {
  debian: "sudo ufw allow 80/tcp && sudo ufw allow 443/tcp",
  rhel: "sudo firewall-cmd --permanent --add-service=http --add-service=https && sudo firewall-cmd --reload",
};

const DOCKER_PORTS = "sudo docker ps --format '{{.Names}}: {{.Ports}}'";

const IF_DNF5 = "if command -v dnf5 >/dev/null 2>&1; then";

// ---------------------------------------------------------------- the recipes

export const RECIPES: Recipe[] = [
  {
    id: "ssh-keys",
    group: "secure",
    title: "Log in with a key instead of a password",
    summary: "Set up an SSH key, then turn password login off so bots can't guess their way in.",
    who: "Anyone who still types a password to log in to this server.",
    fields: [{ id: "publicKey", label: "Your key", kind: "key", help: "The key on this computer that should log in to the server." }],
    steps: [
      {
        title: "Make a key on your computer",
        where: "local",
        skipIfKey: true,
        explain:
          "A key is a pair of files. The private half stays on your computer, like a house key; the public half (the .pub file) goes on servers, like a lock only that key opens. Press Enter to keep the usual place. A passphrase is optional: it locks the key with a password of its own.",
        command: "ssh-keygen -t ed25519",
        expect: "\"Your public key has been saved in\", then a path ending in .pub. If it asks to overwrite, answer n: you already have a key.",
      },
      {
        title: "Put your public key on the server",
        needs: ["publicKey"],
        explain:
          "This adds the public half of your key to ~/.ssh/authorized_keys, the list of keys allowed to log in as you. Run it as the user you log in as. Running it twice is fine: a key that's already there isn't added again.",
        command: addKey,
      },
      {
        title: "Check that the key works",
        where: "local",
        check: true,
        explain: "In a new terminal on your computer, this logs in to the server afresh with your key only, never a password. If your key has a passphrase, it asks for that, which is fine.",
        command: keyLogin,
        expect: "Key login works",
        ifFails: {
          explain:
            "\"Permission denied (publickey)\" means the server didn't take the key. SSH ignores keys when other users could change your home folder or ~/.ssh, so this tightens both on the server. Then check again, and don't go on until it works.",
          command: "chmod go-w ~ && chmod 700 ~/.ssh && chmod 600 ~/.ssh/authorized_keys",
        },
      },
      {
        title: "Turn off password login",
        lockout: true,
        warn: "Only do this once the check above works, or you could lock yourself out. Keep this terminal open until the last check passes: it stays logged in, so you can undo the change from here.",
        explain:
          "This puts two settings in a file of their own, /etc/ssh/sshd_config.d/00-ssheesh.conf. Many cloud servers have a 50-cloud-init.conf next to it that turns password login back on. SSH keeps the first value it reads and reads these files in name order, so 00 wins. sshd -t checks for mistakes before SSH reloads.",
        command: sshdSet("00-ssheesh.conf", ["PasswordAuthentication no", "KbdInteractiveAuthentication no"]),
      },
      {
        title: "Check that password login is off",
        check: true,
        explain: "This asks SSH which settings it really uses after reading every file. Keyboard-interactive is the other way a password can be typed in.",
        command: "sudo sshd -T | grep -iE '^(passwordauthentication|kbdinteractiveauthentication) '",
        expect: "passwordauthentication no and kbdinteractiveauthentication no",
        ifFails: {
          explain: "If either still says yes, a file that's read earlier sets it. This lists every file that mentions them, so you can change the setting there.",
          command: "sudo grep -riE '^\\s*(PasswordAuthentication|KbdInteractiveAuthentication)' /etc/ssh/sshd_config /etc/ssh/sshd_config.d",
        },
      },
      {
        title: "Check you can still log in",
        where: "local",
        check: true,
        explain:
          "In a new terminal on your computer, log in once more with your key. Close the old terminal only after this works. SSHeesh tries your usual keys first, so it keeps signing in; if you picked a key with another name, add the server to SSHeesh again with that key.",
        command: keyLogin,
        expect: "Key login works",
        ifFails: {
          explain: "In the terminal you kept open, this turns password login back on. Then go back to the first check.",
          command: sshdUndo("00-ssheesh.conf"),
        },
      },
    ],
  },

  {
    id: "firewall",
    group: "secure",
    title: "Turn on the firewall",
    summary: "Block every incoming connection you didn't ask for, without cutting yourself off.",
    who: "Any server on the internet that has no firewall turned on yet.",
    fields: [SSH_PORT],
    steps: [
      SEE_SSH_PORT,
      {
        title: "Install the firewall",
        explain: {
          debian:
            "ufw, the \"uncomplicated firewall\", is the usual one on Debian and Ubuntu. Ubuntu comes with it, so there this changes nothing. A firewall in your hosting provider's control panel is separate and can run alongside.",
          rhel: "firewalld is the usual firewall on Fedora and RHEL, and is often installed already. A firewall in your hosting provider's control panel is separate and can run alongside.",
        },
        command: { debian: "sudo apt-get install -y ufw", rhel: "sudo dnf install -y firewalld" },
      },
      {
        title: "Let SSH in first",
        needs: ["sshPort"],
        explain: {
          debian: "A rule says what may come in. Adding the rule for SSH before the firewall is on means turning it on can't cut you off.",
          rhel: "A rule says what may come in. firewalld lets SSH in on port 22 out of the box; this makes sure the rule for your SSH port is there before it's on.",
        },
        command: { debian: (c) => `sudo ufw allow ${c.sshPort}/tcp`, rhel: firewalldAllowSsh },
      },
      {
        title: "Turn the firewall on",
        lockout: true,
        warn: "If the SSH rule is wrong, this blocks new logins. Keep this terminal open: the connection you're using stays up, so you can undo it from here.",
        explain: {
          debian:
            "ufw turns on now and after every restart. It warns that this may disrupt SSH connections and asks whether to go on: SSH is allowed, so type y and press Enter. From now on only what you allowed can come in; the server can still reach out.",
          rhel: "firewalld starts now and after every restart. From now on only what you allowed can come in; the server can still reach out.",
        },
        command: { debian: "sudo ufw enable", rhel: "sudo systemctl enable --now firewalld" },
      },
      {
        title: "Check you can still log in",
        where: "local",
        check: true,
        explain: "In a new terminal on your computer, log in to the server afresh. Close the old terminal only after this works.",
        command: (c) => c.ssh({ run: "echo Still in" }),
        expect: "Still in",
        ifFails: {
          explain: "In the terminal you kept open, this turns the firewall off again. Then check the SSH port and try once more.",
          command: { debian: "sudo ufw disable", rhel: "sudo systemctl disable --now firewalld" },
        },
      },
      {
        title: "See what's allowed in",
        check: true,
        explain: "This lists the firewall's rules, so you can see what's open.",
        command: { debian: "sudo ufw status verbose", rhel: "sudo firewall-cmd --list-all" },
        expect: {
          debian: (c) => `Status: active, and ${c.sshPort}/tcp with ALLOW IN`,
          rhel: (c) => (c.sshPort === 22 ? "ssh on the services line" : `${c.sshPort}/tcp on the ports line`),
        },
      },
      {
        title: "Let web traffic in, if this is a web server",
        explain:
          "Websites use port 80 (http) and 443 (https). Skip this if the server doesn't serve a website. Open other ports only if the whole internet should reach them; a database usually shouldn't.",
        command: WEB_IN,
      },
      {
        title: "Know that Docker goes around it",
        explain:
          "Ports that Docker publishes (docker run -p, or ports: in docker-compose.yml) skip the firewall and stay open to everyone. This lists them: 0.0.0.0:5432->5432/tcp is open to the whole internet, 127.0.0.1:5432->5432/tcp only to this server. To make one private, publish it as 127.0.0.1:5432:5432. Skip this if you don't use Docker.",
        command: DOCKER_PORTS,
      },
    ],
  },

  {
    id: "fail2ban",
    group: "secure",
    title: "Stop password guessing with fail2ban",
    summary: "Block an address for an hour after it fails to log in a few times.",
    who: "Servers that still allow password login, or whose logs fill up with failed logins.",
    fields: [SSH_PORT],
    steps: [
      SEE_SSH_PORT,
      {
        title: "Install fail2ban",
        explain: {
          debian:
            "fail2ban watches for failed logins and blocks an address that keeps failing. python3-systemd lets it read them from the system journal, where newer Debian keeps them.",
          rhel: "fail2ban watches for failed logins and blocks an address that keeps failing. Fedora has it built in. Rocky Linux, AlmaLinux and CentOS Stream get it from EPEL, a widely used extra package collection, so this adds EPEL first. On Red Hat's own RHEL, set up EPEL with Red Hat's guide instead.",
        },
        command: {
          debian: "sudo apt-get update && sudo apt-get install -y fail2ban python3-systemd",
          rhel: "{ grep -q '^ID=fedora' /etc/os-release || sudo dnf install -y epel-release; } && sudo dnf install -y fail2ban python3-systemd",
        },
      },
      {
        title: "Tell it to guard SSH",
        needs: ["sshPort"],
        explain: {
          debian:
            "This writes the rules to a file of their own: an address that fails 5 logins within 10 minutes is blocked for an hour. That includes you, if you mistype your password too often.",
          rhel: "This writes the rules to a file of their own: an address that fails 5 logins within 10 minutes is blocked for an hour. That includes you, if you mistype your password too often. fail2ban blocks through firewalld here, so turn the firewall on first.",
        },
        command: (c) =>
          `printf '[sshd]\\nenabled = true\\nport = ${c.sshPort}\\nbackend = systemd\\nmaxretry = 5\\nfindtime = 10m\\nbantime = 1h\\n' | sudo tee /etc/fail2ban/jail.d/ssheesh.local >/dev/null`,
      },
      {
        title: "Start it",
        explain: "This starts fail2ban with the new rules, now and after every restart.",
        command: "sudo systemctl enable fail2ban && sudo systemctl restart fail2ban",
      },
      {
        title: "Check it's watching",
        check: true,
        explain:
          "If it can't reach the socket, fail2ban is still starting: wait a few seconds and try again. To unblock an address later, for example 203.0.113.7, run sudo fail2ban-client set sshd unbanip 203.0.113.7.",
        command: "sudo fail2ban-client status sshd",
        expect: "Status for the jail: sshd, with counts of failed logins and blocked addresses",
        ifFails: { explain: "fail2ban's own log says why it didn't start.", command: "sudo journalctl -u fail2ban -n 30 --no-pager" },
      },
    ],
  },

  {
    id: "auto-updates",
    group: "secure",
    title: "Install security updates automatically",
    summary: "Let the server patch known security holes every day, without you.",
    who: "Nearly every server, unless someone updates it by hand every week.",
    steps: [
      {
        title: "Install unattended-upgrades",
        only: "debian",
        explain: "unattended-upgrades installs updates with nobody logged in. Out of the box it only installs security updates, which rarely break anything. Ubuntu usually has it already.",
        command: "sudo apt-get update && sudo apt-get install -y unattended-upgrades",
      },
      {
        title: "Run it every day",
        only: "debian",
        explain: "This tells apt to refresh its package lists and run unattended-upgrades once a day. It's the same file Debian's own setup writes.",
        command: `printf 'APT::Periodic::Update-Package-Lists "1";\\nAPT::Periodic::Unattended-Upgrade "1";\\n' | sudo tee /etc/apt/apt.conf.d/20auto-upgrades >/dev/null`,
      },
      {
        title: "Check the setting",
        only: "debian",
        check: true,
        explain: "This shows the two settings apt actually uses, after reading all its files.",
        command: "apt-config dump | grep -E '^APT::Periodic::(Update-Package-Lists|Unattended-Upgrade) '",
        expect: "Both lines end in \"1\";",
      },
      {
        title: "Install dnf-automatic",
        only: "rhel",
        explain: "dnf-automatic installs updates with nobody logged in. Newer Fedora calls it dnf5-plugin-automatic, so this picks the right name.",
        command: `${IF_DNF5} sudo dnf install -y dnf5-plugin-automatic; else sudo dnf install -y dnf-automatic; fi`,
      },
      {
        title: "Have it install security updates",
        only: "rhel",
        explain: "Out of the box it only downloads updates. This makes it install them, limited to security updates, which rarely break anything.",
        command: `${IF_DNF5} printf '[commands]\\nupgrade_type = security\\napply_updates = yes\\n' | sudo tee /etc/dnf/automatic.conf >/dev/null; else sudo sed -i -e 's/^upgrade_type *=.*/upgrade_type = security/' -e 's/^apply_updates *=.*/apply_updates = yes/' /etc/dnf/automatic.conf; fi`,
      },
      {
        title: "Run it every day",
        only: "rhel",
        explain: "A timer runs it once a day, starting now and after every restart.",
        command: `${IF_DNF5} sudo systemctl enable --now dnf5-automatic.timer; else sudo systemctl enable --now dnf-automatic.timer; fi`,
      },
      {
        title: "Check the timer",
        check: true,
        explain:
          "This shows when updates run next. Some updates, like a new kernel, only take effect after a restart; SSHeesh's security check-up tells you when one is waiting.",
        command: { debian: "systemctl list-timers apt-daily-upgrade.timer", rhel: "systemctl list-timers 'dnf*automatic*'" },
        expect: "A timer with a time in the NEXT column",
      },
    ],
  },

  {
    id: "sudo-user",
    group: "secure",
    title: "Use your own user instead of root",
    summary: "Make a user that runs admin commands with sudo, then stop root from logging in over SSH.",
    who: "Anyone who logs in to this server as root.",
    fields: [
      {
        id: "newUser",
        label: "Name for the new user",
        kind: "text",
        placeholder: "alex",
        help: "Lowercase, like alex or sam. You'll log in with it from now on.",
        pattern: /^(?!(root|daemon|bin|sys|nobody|admin|sudo|wheel|docker|caddy)$)[a-z][a-z0-9_-]{0,31}$/,
        invalid: "Lowercase letters, digits, - and _, starting with a letter, and not a name the system already uses.",
      },
    ],
    steps: [
      {
        title: "Make sure sudo is there",
        explain:
          "sudo lets a normal user run one command as root, after typing their own password. Most servers have it; a minimal Debian doesn't. If it's missing, you must be root, so this installs it without sudo.",
        command: { debian: "command -v sudo >/dev/null || apt-get install -y sudo", rhel: "command -v sudo >/dev/null || dnf install -y sudo" },
      },
      {
        title: "Create the user",
        needs: ["newUser"],
        explain: {
          debian: "It asks for the new user's password twice: pick a long one, you'll type it for sudo. It then asks for a full name and such; press Enter to skip those.",
          rhel: "It then asks for the new user's password twice: pick a long one, you'll type it for sudo.",
        },
        command: { debian: (c) => `sudo adduser ${c.values.newUser}`, rhel: (c) => `sudo useradd -m ${c.values.newUser} && sudo passwd ${c.values.newUser}` },
      },
      {
        title: "Let it use sudo",
        needs: ["newUser"],
        explain: {
          debian: "Members of the sudo group may run commands as root after typing their own password.",
          rhel: "Members of the wheel group may run commands as root after typing their own password.",
        },
        command: { debian: (c) => `sudo usermod -aG sudo ${c.values.newUser}`, rhel: (c) => `sudo usermod -aG wheel ${c.values.newUser}` },
      },
      {
        title: "Give it your key",
        needs: ["newUser"],
        explain:
          "This copies the keys that log you in now to the new user, so the same key works for both. If you log in with a password, skip this: the new user logs in with the password you just set.",
        command: {
          debian: (c) => {
            const u = c.values.newUser;
            return `sudo install -d -m 700 -o ${u} -g ${u} /home/${u}/.ssh && sudo install -m 600 -o ${u} -g ${u} ~/.ssh/authorized_keys /home/${u}/.ssh/authorized_keys`;
          },
          rhel: (c) => {
            const u = c.values.newUser;
            return `sudo install -d -m 700 -o ${u} -g ${u} /home/${u}/.ssh && sudo install -m 600 -o ${u} -g ${u} ~/.ssh/authorized_keys /home/${u}/.ssh/authorized_keys && sudo restorecon -R /home/${u}/.ssh`;
          },
        },
      },
      {
        title: "Check the new user can log in and use sudo",
        where: "local",
        check: true,
        needs: ["newUser"],
        explain: "In a new terminal on your computer, this logs in as the new user and runs one command with sudo, which asks for the new user's password.",
        command: (c) => c.ssh({ user: c.values.newUser, tty: true, run: "sudo whoami" }),
        expect: "root",
        ifFails: {
          explain: "This shows the new user's groups on the server: sudo (or wheel on Fedora and RHEL) should be among them. Don't go on until the check works.",
          command: (c) => `id ${c.values.newUser}`,
        },
      },
      {
        title: "Stop root logging in over SSH",
        lockout: true,
        warn: "Only do this once the check above works. Keep this terminal open until the last check passes, so you can undo it from here.",
        explain:
          "From now on you log in as the new user and put sudo in front of admin commands. This writes PermitRootLogin no to a file of its own, checks it with sshd -t and reloads SSH. If SSHeesh logs in to this server as root, add the server again with the new user's name.",
        command: sshdSet("00-ssheesh-root.conf", ["PermitRootLogin no"]),
      },
      {
        title: "Check root is turned away",
        check: true,
        explain: "This asks SSH which setting it really uses after reading every file.",
        command: "sudo sshd -T | grep -i '^permitrootlogin '",
        expect: "permitrootlogin no",
      },
      {
        title: "Check you can still get in",
        where: "local",
        check: true,
        needs: ["newUser"],
        explain: "In a new terminal on your computer, log in as the new user once more. Close the old terminal only after this works.",
        command: (c) => c.ssh({ user: c.values.newUser, run: "echo Still in" }),
        expect: "Still in",
        ifFails: { explain: "In the terminal you kept open, this lets root log in again.", command: sshdUndo("00-ssheesh-root.conf") },
      },
    ],
  },

  {
    id: "docker",
    group: "setup",
    title: "Install Docker",
    summary: "Install Docker from Docker's own package source, so you get current versions and their updates.",
    who: "Anyone who wants to run apps in containers on this server.",
    steps: [
      {
        title: "Remove other Docker packages",
        only: "debian",
        explain:
          "Debian and Ubuntu have Docker packages of their own that clash with Docker's. This removes the ones that are installed; if apt asks, answer Y. Images and volumes in /var/lib/docker stay.",
        command: "sudo apt-get remove $(dpkg --get-selections docker.io docker-compose docker-compose-v2 docker-doc podman-docker containerd runc 2>/dev/null | cut -f1)",
      },
      {
        title: "Add Docker's signing key",
        only: "debian",
        explain: "apt checks every package against its source's signing key. This downloads Docker's key, for Debian or Ubuntu as the server is.",
        command:
          'sudo apt-get update && sudo apt-get install -y ca-certificates curl && sudo install -m 0755 -d /etc/apt/keyrings && sudo curl -fsSL "https://download.docker.com/linux/$(. /etc/os-release && echo "$ID")/gpg" -o /etc/apt/keyrings/docker.asc && sudo chmod a+r /etc/apt/keyrings/docker.asc',
      },
      {
        title: "Add Docker's package source",
        only: "debian",
        explain: "This tells apt where Docker's packages are, matched to your system's release.",
        command:
          "printf 'Types: deb\\nURIs: https://download.docker.com/linux/%s\\nSuites: %s\\nComponents: stable\\nSigned-By: /etc/apt/keyrings/docker.asc\\n' " +
          '"$(. /etc/os-release && echo "$ID")" "$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")" ' +
          "| sudo tee /etc/apt/sources.list.d/docker.sources >/dev/null",
      },
      {
        title: "Add Docker's package source",
        only: "rhel",
        explain: "This downloads the file that tells dnf where Docker's packages are: Docker's Fedora or RHEL source, or the CentOS one, which Rocky Linux and AlmaLinux use.",
        command:
          'sudo curl -fsSL -o /etc/yum.repos.d/docker-ce.repo "https://download.docker.com/linux/$(. /etc/os-release; case "$ID" in fedora|rhel) echo "$ID" ;; *) echo centos ;; esac)/docker-ce.repo"',
      },
      {
        title: "Install Docker",
        explain: {
          debian: "Installs Docker, with Compose for docker-compose.yml files and Buildx for building images, and starts it now and after every restart.",
          rhel: "Installs Docker, with Compose for docker-compose.yml files and Buildx for building images, and starts it now and after every restart. If dnf says it clashes with podman-docker, remove that first with sudo dnf remove podman-docker.",
        },
        command: {
          debian: "sudo apt-get update && sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin && sudo systemctl enable --now docker",
          rhel: "sudo dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin && sudo systemctl enable --now docker",
        },
      },
      {
        title: "Check that Docker works",
        check: true,
        explain: "This downloads a tiny test image and runs it once.",
        command: "sudo docker run --rm hello-world",
        expect: "Hello from Docker!",
      },
      {
        title: "Use docker without sudo",
        warn: "Anyone in the docker group can take full control of the server, the same as root. Only add users you'd trust with root.",
        explain: "This puts you in the docker group, so you can type docker instead of sudo docker. Skip it if you log in as root.",
        command: 'sudo usermod -aG docker "$USER"',
      },
      {
        title: "Pick up the new group",
        explain:
          "A new group only reaches new logins, and SSHeesh keeps your terminal running between connections. newgrp starts a shell in this terminal that has the group already; type exit to leave it.",
        command: "newgrp docker",
      },
      {
        title: "Check docker works without sudo",
        check: true,
        explain: "The same test as before, this time without sudo.",
        command: "docker run --rm hello-world",
        expect: "Hello from Docker!",
        ifFails: {
          explain: "\"permission denied\" means this shell doesn't have the group yet. This lists your groups: run newgrp docker if docker isn't among them.",
          command: "id -nG",
        },
      },
      {
        title: "Keep containers' ports private",
        explain:
          "Ports Docker publishes skip the firewall and are open to the whole internet. In docker-compose.yml, write a port as \"127.0.0.1:8080:80\" so only this server reaches it, and put Caddy in front for the web (see the HTTPS recipe). This lists what's published now: 0.0.0.0 means open to everyone.",
        command: DOCKER_PORTS,
      },
    ],
  },

  {
    id: "swap",
    group: "setup",
    title: "Add swap",
    summary: "Give the server spare memory on disk, so a busy moment slows apps down instead of crashing them.",
    who: "Small servers, with 2 GB of memory or less, that have no swap yet.",
    fields: [
      {
        id: "swapSize",
        label: "Swap size",
        kind: "choice",
        help: "As much as the server's memory is plenty; 2 GB suits most small servers.",
        choices: [
          { value: "1", label: "1 GB" },
          { value: "2", label: "2 GB" },
          { value: "4", label: "4 GB" },
          { value: "8", label: "8 GB" },
        ],
        initial: "2",
      },
    ],
    steps: [
      {
        title: "See memory, swap and free space",
        explain:
          "free shows memory (Mem) and swap (Swap); a swap file takes disk space, so check / has room for it. Fedora often has swap already, as zram (compressed memory), and may not need more.",
        command: "free -h && swapon --show && df -h /",
        expect: "Swap: 0B means there's no swap yet",
      },
      {
        title: "Create the swap file",
        needs: ["swapSize"],
        explain:
          "This makes /swapfile, readable only by root, and sets it up as swap. Filling it can take a minute. btrfs, the file system Fedora uses, needs its own command for swap files, so this picks the right one. If /swapfile exists, it leaves it alone.",
        command: (c) => {
          const gb = Number(c.values.swapSize);
          return `if [ -e /swapfile ]; then echo "/swapfile already exists"; elif [ "$(stat -f -c %T /)" = btrfs ]; then sudo btrfs filesystem mkswapfile --size ${gb}G /swapfile; else sudo dd if=/dev/zero of=/swapfile bs=1M count=${gb * 1024} status=progress && sudo chmod 600 /swapfile && sudo mkswap /swapfile; fi`;
        },
      },
      {
        title: "Turn it on",
        explain: "The server starts using it right away. If it says the device is busy, it's on already.",
        command: "sudo swapon /swapfile",
      },
      {
        title: "Keep it on after a restart",
        explain: "This adds /swapfile to /etc/fstab, the list of disks and files set up at startup, unless it's there already.",
        command: "grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap defaults 0 0' | sudo tee -a /etc/fstab >/dev/null && sudo systemctl daemon-reload",
      },
      {
        title: "Check the swap",
        check: true,
        explain: "The same look as at the start.",
        command: "swapon --show && free -h",
        expect: (c) => `/swapfile with a size of ${c.values.swapSize ?? "2"}G, and swap in free's Swap line`,
      },
    ],
  },

  {
    id: "caddy-https",
    group: "setup",
    title: "Put a web app behind HTTPS with Caddy",
    summary: "Serve an app that runs on this server at your own domain, with a free HTTPS certificate that renews itself.",
    who: "Anyone with an app on a local port and a domain name to point at this server.",
    fields: [
      {
        id: "domain",
        label: "Domain",
        kind: "text",
        placeholder: "example.com",
        help: "The name people type to reach your app, like example.com or app.example.com.",
        pattern: /^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]{0,61}[a-z0-9]$/i,
        invalid: "A domain like example.com or app.example.com, without https:// or a path.",
      },
      {
        id: "appPort",
        label: "Port your app listens on",
        kind: "text",
        placeholder: "3000",
        help: "Often 3000 for Node apps, 8000 for Python, 8080 for Java.",
        pattern: PORT,
        invalid: "A number from 1 to 65535.",
      },
    ],
    steps: [
      {
        title: "Point your domain at this server",
        needs: ["domain"],
        explain:
          "Where you bought the domain, or at your DNS provider, add an A record for it with this server's public IP address (and an AAAA record if the server has IPv6). This shows where the domain points now. Changes can take a few minutes to show.",
        command: (c) => `getent ahosts ${c.values.domain} | awk '{print $1}' | sort -u`,
        expect: "This server's public address. Nothing at all means the domain doesn't point anywhere yet.",
      },
      {
        title: "Let web traffic in",
        explain: "Caddy needs ports 80 and 443 open, both to get a certificate and to serve your app. Skip this if no firewall is on.",
        command: WEB_IN,
      },
      {
        title: "Add Caddy's package source",
        only: "debian",
        explain: "Caddy is a web server that gets and renews HTTPS certificates by itself. This adds the Caddy project's own package source and signing key, as Caddy's install guide does.",
        command:
          "sudo apt-get update && sudo apt-get install -y curl gpg && curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg && curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null && sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list",
      },
      {
        title: "Install Caddy",
        explain: {
          debian: "This installs Caddy and starts it, now and after every restart. It runs as its own user, caddy, not as root.",
          rhel: "Caddy is a web server that gets and renews HTTPS certificates by itself. It comes from COPR, Fedora's community package service, where the Caddy team publishes it, as Caddy's install guide says. This installs it and starts it, now and after every restart.",
        },
        command: {
          debian: "sudo apt-get update && sudo apt-get install -y caddy",
          rhel: "sudo dnf install -y 'dnf-command(copr)' && sudo dnf copr enable -y @caddy/caddy && sudo dnf install -y caddy && sudo systemctl enable --now caddy",
        },
      },
      {
        title: "Send your domain to the app",
        needs: ["domain", "appPort"],
        warn: "This replaces /etc/caddy/Caddyfile, keeping the first one as Caddyfile.orig. If Caddy already serves other sites here, add these lines to the file instead.",
        explain:
          "Caddy's settings file gets three lines: requests for your domain go to your app on this server. Your app only needs to listen on localhost; Caddy faces the internet and handles HTTPS.",
        command: (c) =>
          `[ -e /etc/caddy/Caddyfile.orig ] || sudo cp /etc/caddy/Caddyfile /etc/caddy/Caddyfile.orig; printf '%s {\\n\\treverse_proxy localhost:%s\\n}\\n' '${c.values.domain}' '${c.values.appPort}' | sudo tee /etc/caddy/Caddyfile >/dev/null`,
      },
      {
        title: "Load the new settings",
        explain:
          "Caddy checks the new settings first and keeps the old ones running if something's wrong. The first time, it gets a certificate for your domain, which takes a few seconds.",
        command: "sudo systemctl reload caddy",
      },
      {
        title: "Check the site",
        check: true,
        needs: ["domain"],
        explain: "This asks your domain for the page over HTTPS and shows the first line of the answer.",
        command: (c) => `curl -sSI https://${c.values.domain} | head -n 1`,
        expect: "HTTP/2 200, or another code your app sends. 502 means Caddy works but nothing answers on your app's port.",
        ifFails: {
          explain: "Caddy's log says why, most often that the domain doesn't point here yet or ports 80 and 443 are closed.",
          command: "sudo journalctl -u caddy -n 30 --no-pager",
        },
      },
    ],
  },
];

export function recipeById(id: string | null | undefined): Recipe | undefined {
  return RECIPES.find((r) => r.id === id);
}

/** The recipe that fixes a security check-up finding, by the check's id. */
export const RECIPE_FOR_CHECK: Record<string, string> = {
  "password-login": "ssh-keys",
  "root-login": "sudo-user",
  firewall: "firewall",
  "brute-force": "fail2ban",
  "auto-updates": "auto-updates",
};
