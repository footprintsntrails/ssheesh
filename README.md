<div align="center">

<img src="docs/banner.gif" alt="ssheesh: servers, terminals and files that don't fall over">

[![Latest release](https://img.shields.io/github/v/release/footprintsntrails/ssheesh?include_prereleases&label=release&color=4eba65)](https://github.com/footprintsntrails/ssheesh/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/footprintsntrails/ssheesh/ci.yml?branch=main&label=build)](https://github.com/footprintsntrails/ssheesh/actions/workflows/ci.yml)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/license-CC%20BY--NC--SA%204.0-6b717b)](LICENSE)
![macOS · Windows · Linux](https://img.shields.io/badge/macOS%20·%20Windows%20·%20Linux-1f1e1d)

[**Download**](https://github.com/footprintsntrails/ssheesh/releases/latest) · [Install on macOS](docs/install-macos.md) · [Install on Windows](docs/install-windows.md) · [Request a feature](https://github.com/footprintsntrails/ssheesh/issues/new/choose)

</div>

> [!WARNING]
> **Version 0.8 is an early preview.** Plenty is still missing and things will break. Try it, and [say what's wrong](https://github.com/footprintsntrails/ssheesh/issues/new/choose).

<p align="center"><img src="docs/screenshots/workspace.png" alt="SSHeesh: a server's terminal with the log it printed opened beside it, its tabs along the top, and an AI agent that needs you"></p>

SSHeesh is a calm window for your servers: their terminals, files, containers and AI agents, laid out like a browser. Each server is like a site with tabs of its own, and the tabs of every server you have open sit in one row, each saying whose it is: “Files (web-1)”. It checks your servers' security and frees their disks with one click each.

## The idea

**Simple, and the screen is yours.** The terminal gets the room. The sidebar is a slim row of server icons, the tabs share one bar with the server you're on, and a button shows up when you reach for it, not before. A compact mode hides the sidebar, the tab bar or both. Nothing to learn first: it reads like a browser.

**Your servers stay yours.** No account and no cloud: hosts, keys and passwords stay on your computer, and passwords live in your system's keychain. SSHeesh talks to your servers, and to GitHub to check for updates. Tokens, keys and passwords in a terminal's output show as dots until you click them, and **streamer mode** hides server names, addresses and users everywhere, terminal output included, so you can share your screen or stream without a second thought.

**It's convenient from the first minute.** Paste a whole `ssh` command and every field fills in; your `~/.ssh/config`, keys and agent just work. Errors come in plain words, and every command SSHeesh suggests says what it does. A dropped Wi-Fi doesn't end your work: with `tmux` on the server, your session keeps running and SSHeesh tells you what you missed, and uploads and downloads pick up where they stopped.

**You make it yours.** SSHeesh Default, Claude's warm dark with the >_< logo's green, Midnight and three more themes you can edit down to every color, any accent color and your own terminal colors, your own keyboard shortcuts, tabs along the top or under each server, one tab at a time or side by side, a compact mode that hides the sidebar, the tab bar or both, icons in lines or pixels (in any color) or your own picture for every server, and the welcome a server greets you with. Change it all in the first minute, with a live preview, or later in Settings.

<p align="center"><img src="docs/media/onboarding.gif" alt="The first-run setup: picking servers from ~/.ssh/config, then a theme, terminal colors and a layout with a live preview of the app"></p>

## Keep a server safe and tidy

Two buttons at the foot of the sidebar, over Settings, look at the server you have open (with several open, you pick one).

- **Security check-up** (the shield). Password logins, root login, open ports, the firewall, Docker ports that go around it, automatic updates, waiting security fixes, brute-force protection and a restart that's due, each in plain words with what it means for you. Looking changes nothing. A fix runs when you press **Run**, on the server, without a terminal, and you see how it went. When it needs you (sudo asks for your password, or the package manager asks whether to go ahead), it opens in a terminal instead and says exactly what to type.
- **Free up disk space** (the disk). What fills each disk, the biggest folders, and the cleanups that are safe: unused Docker images and build cache, old system logs, package caches, old temporary files, each with how much it frees. **Run** does it.

<p align="center"><img src="docs/screenshots/checkup.png" alt="The security check-up: what to fix, what's worth a look and what's good, each with a Run button" width="560"></p>

## Tricks worth knowing

- **Peek at a file.** Paths in a terminal's output are faintly underlined: click one and the file opens beside the terminal, at the lines it names (`app.ts:12-40`), Markdown nicely formatted. A path an agent prints from its own project is found too.

<p align="center"><img src="docs/media/peek.gif" alt="Clicking a path in an agent's output opens the file beside the terminal"></p>

- **Drag a tab onto the page** to see two at once, also two servers' tabs side by side; its × (or dragging it back to the tabs) makes it a tab again, still running. Ctrl+Shift+S switches between one tab at a time and side by side.

<p align="center"><img src="docs/media/split.gif" alt="Dragging the Files tab onto the page splits it beside the terminal"></p>

- **Copy files from one server to another.** Drag them from one server's Files onto another's beside it, or right-click them and pick the server and the folder there. They go straight through SSHeesh, so the servers don't need to reach each other, and nothing is replaced without asking.
- **Agents on every server, at a glance.** Claude Code, Codex, OpenCode and OMP show up by name on their tab, like “Terminal 2 (Claude Code)”, and the server says “needs you” when one waits for an answer, with a notification if SSHeesh is in the background.
- **A welcome that says where you are.** A new terminal shows the server's own welcome, or SSHeesh's: the system, uptime, memory and disk, containers and updates, beside the logo. The server's notices about updates always stay.

<p align="center"><img src="docs/screenshots/welcome.png" alt="SSHeesh's welcome in a new terminal: the logo beside the server's system, uptime, memory and disk, containers and updates, with the server's own update notices under it"></p>

- **The main menu** (the logo at the top left) is a start page with all your servers, what's running and the tabs you closed. Ctrl+Shift+Z brings back the last one.
- **One bar for everything.** Ctrl+Shift+L (⌘L) finds servers, folders, commands and settings, and says which server a command runs on.
- **Your keys.** Every shortcut is listed in Settings → Keyboard, and you can change any of them.
- **Server icons that mean something.** SSHeesh sees what a server runs (a website, a database, Docker) and picks its icon; each server gets a color of its own. Right-click a server for 30 more, in lines or pixels, in any color.
- **Everything about a server, in its tabs:** a quick file manager with bookmarks, uploads and downloads (a download opens its folder with the file picked out), its Docker containers, stats, web apps opened through your SSH login, and step-by-step recipes for the usual jobs, whose steps run in the terminal with **Run**.

<p align="center"><img src="docs/media/streamer.gif" alt="Streamer mode hides server names and addresses across the app and the terminal"></p>

## Install

| | |
| --- | --- |
| **macOS** 11+ (Apple Silicon and Intel) | One command in Terminal (below), or download the `.dmg` · [Guide](docs/install-macos.md) |
| **Windows** 10 / 11 | Download `SSHeesh.exe` and run it, nothing to install. Or the `setup.exe`. Both update themselves · [Guide](docs/install-windows.md) |
| **Linux** | Download the `.AppImage`: one file, nothing to install. Make it runnable once (`chmod +x`), then double-click it. |

**Quickest on a Mac:** paste this into Terminal. It installs the latest version and opens it, with no security prompts:

```bash
curl -fsSL https://raw.githubusercontent.com/footprintsntrails/ssheesh/main/scripts/install-macos.sh | sh
```

All files are on the [Releases page](https://github.com/footprintsntrails/ssheesh/releases/latest). The app isn't signed by Apple or Microsoft yet, so the first launch needs a few extra clicks; the guides show every step.

**On your servers:** any Linux works. Install `tmux` for sessions that survive disconnects.

## Ideas and bugs

Feature requests are very welcome: [open one](https://github.com/footprintsntrails/ssheesh/issues/new?template=feature_request.yml). SSHeesh is built to one vision, so pull requests aren't accepted; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Build from source

You need [Node.js](https://nodejs.org) 22+, [Rust](https://rustup.rs) and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev      # run the app
npm run dev            # just the interface, in a browser, with sample data
npm run tauri build    # make an installer
```

| Path | What's there |
| --- | --- |
| `crates/sshee-core` | Rust core: hosts, sessions that reconnect, files, metrics. No UI. |
| `src-tauri` | The desktop app shell; exposes the core to the window. |
| `src` | The interface: React, Tailwind, xterm.js. |

Everything remote runs through the system `ssh` with connection sharing, so files and stats reuse the terminal's login. Durable sessions are named `tmux` sessions on the server.

## Roadmap

What's planned next. The order may still change.

- **0.9: Docker and services.** Compose stacks with per-container stats, and updates you can roll back. systemd services, and version history for config files.
- **0.10: workspaces.** A workspace per project, a drop-down terminal and a tray monitor. App lock, importing from other SSH apps, and `ssh://` links.
- **0.11: sync.** Encrypted sync through a folder you pick, with no account.
- **Along the way:** signed builds for macOS and Windows, and a built-in SSH connection on Windows, since its `ssh` can't share connections.

See what changed in each version in the [changelog](CHANGELOG.md).

## Credits

Made by [@footprintsntrails](https://github.com/footprintsntrails), built together with [Claude](https://claude.com/claude-code) by Anthropic.

Terminal color schemes come from their open-source authors: Tokyo Night, Catppuccin, Dracula, Nord, One Dark, Gruvbox and Solarized.

Icons in prompts like oh-my-posh and Starship come from the [Nerd Fonts](https://www.nerdfonts.com) symbols font (MIT; each icon set under its own open license). Server icons are [Phosphor](https://phosphoricons.com) (MIT) and [pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann (MIT).

## License

[CC BY-NC-SA 4.0](LICENSE). The code is open: you may use, study, share and modify SSHeesh for **non-commercial** purposes, as long as you credit the project and release anything you build on it under the same license, with its source open. Commercial use isn't allowed without permission.
