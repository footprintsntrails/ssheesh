<div align="center">

<img src="docs/banner.gif" alt="ssheesh: servers, terminals and files that don't fall over">

[![Latest release](https://img.shields.io/github/v/release/footprintsntrails/ssheesh?include_prereleases&label=release&color=4eba65)](https://github.com/footprintsntrails/ssheesh/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/footprintsntrails/ssheesh/ci.yml?branch=main&label=build)](https://github.com/footprintsntrails/ssheesh/actions/workflows/ci.yml)
[![License: CC BY-NC-SA 4.0](https://img.shields.io/badge/license-CC%20BY--NC--SA%204.0-6b717b)](LICENSE)
![macOS · Windows · Linux](https://img.shields.io/badge/macOS%20·%20Windows%20·%20Linux-1f1e1d)

[**Download**](https://github.com/footprintsntrails/ssheesh/releases/latest) · [Install](#install) · [Request a feature](https://github.com/footprintsntrails/ssheesh/issues/new/choose)

</div>

> [!WARNING]
> **SSHeesh 1.0 is rebuilt from the ground up**, so some things will be rough or still missing. If something's wrong, [please say so](https://github.com/footprintsntrails/ssheesh/issues/new/choose).

<p align="center"><img src="docs/screenshots/1.0/hero.png" alt="SSHeesh with the sidebar listing four servers by name, a terminal on web-7731 and its Files side by side, and a row of tabs on top where an agent on db-main says Needs you"></p>

SSHeesh is a calm window for your servers: their terminals, files, containers and AI agents, laid out like a browser. Each server is like a site with tabs of its own, and the tabs of every server you have open sit in one row, each with its server's name beside it. It checks your servers' security and frees their disks with one click each, and with one more it hides where you connect from, through Tor.

## What makes it different

### Connect through Tor, in one click

Press the onion above Settings and everything goes through Tor: terminals, files, stats and tunnels. Your servers and their logs see a Tor address instead of yours, and your internet provider sees Tor, not which servers you use. If Tor can't carry a connection, the connection isn't made: nothing slips around it, not even a name lookup. Tor is built in, so there's nothing to install, and terminals in `tmux` move over without losing their place.

It's also fast, for Tor. Before a server's first connection, SSHeesh times six Tor routes to it, three times each, and keeps the quickest; the status bar shows the round trip. In our tests from a datacenter a keystroke came back in 60 to 100 ms, a login took about a second, and no pick landed on a route slower than 400 ms. Your own connection adds its share. For that speed ssh doesn't hide your typing rhythm under Tor: your provider, Tor's relays and the server's network can see when you press keys, which can show how long a typed password is and hint at what you typed, though they can't read it. Settings → Tor hides it again, at about twice the delay per key.

<p align="center"><img src="docs/screenshots/1.0/tor.png" alt="The Tor card open over the sidebar's onion: Tor is on with an 84 ms round trip, the last lines of its log, and buttons for new routes and the log file; an onion sits beside the server's name and Tor 84 ms in the status bar"></p>

### Wi-Fi drops, your work doesn't

With `tmux` on the server, a dropped connection only pauses your view: the shell, the deploy, the agent keep running there. A key or Reconnect takes you back into the same session, or SSHeesh reconnects by itself if you tell it to. A line in the terminal then says what you missed while you were away: how many new lines, and how many of them errors and warnings. Uploads and downloads pick up where they stopped, and a terminal you haven't touched for 20 minutes lets go of its connection, then comes back with a key.

<p align="center"><img src="docs/screenshots/1.0/reconnect.png" alt="A terminal back online after a dropped connection: Back online at the top, and a line that says While you were away (41 min): 2 new lines, 1 error, above the error the worker printed meanwhile"></p>

### Made for AI agents on your servers

Claude Code, Codex, OpenCode and OMP show up by name on their tab, and the tab says “Needs you” when one waits for an answer, with a notification if SSHeesh is in the background. Point at that tab to read the question and answer it with a click. Click any path an agent prints and the file opens beside it, at the lines it names (`app.ts:12-40`), Markdown nicely formatted.

<p align="center"><img src="docs/screenshots/1.0/agent.png" alt="The pointer on an agent's tab that says Needs you: a card under it shows Claude Code on db-main waiting 2 minutes, the psql command it wants to run, and its three answers to click"></p>

### Secrets stay off your screen

Tokens, keys and passwords in a terminal's output show as dots before they're ever drawn, while the settings around them stay readable; click one to see it.

### A security check-up and a tidy disk, one click each

The shield at the foot of the sidebar looks at password logins, root login, open ports, the firewall, Docker ports that go around it, updates, brute-force protection and a restart that's due, each in plain words. Looking changes nothing. Each fix shows the exact command first and runs only after you say yes, twice on a server labelled Production; when it needs you (sudo asks for your password), it opens in a terminal. The disk button beside it shows what fills each disk and the cleanups that are safe (Docker leftovers, old logs, package caches, old temporary files), each with how much it frees.

<p align="center">
<img src="docs/screenshots/1.0/checkup.png" width="49%" alt="The security check on web-7731: 2 to fix, 2 worth a look, 3 good; the password login fix shows its command, and since the server is labelled Production it asks a second time with Cancel and Run on Production">
<img src="docs/screenshots/1.0/disk.png" width="49%" alt="Free up disk space on web-7731: the root disk 89% full and a backups disk at 42%, then safe cleanups with what each frees: unused Docker images 2.6 GB, Docker build cache 3.7 GB, old system logs 1.4 GB, each with its command, Run and Copy">
</p>

### Lay it out your way

Tabs on top or under each server, one tab at a time or several side by side, and a compact mode that hides the sidebar, the tab bar or both until you point at the window's edge. Shift+click a server to open it in a window of its own. Mix them however you like.

<p align="center">
<img src="docs/screenshots/1.0/layout-tabs-on-top.png" width="49%" alt="Tabs on top: a narrow sidebar of server icons, and the tabs of three servers in one row along the top">
<img src="docs/screenshots/1.0/layout-tabs-under-servers.png" width="49%" alt="Tabs under each server: the sidebar lists servers by name, with each server's tabs hanging under it, and the agent on db-main marked Needs you">
</p>

## The idea

**Simple, and the screen is yours.** The terminal gets the room, and a button shows up when you reach for it, not before. Nothing to learn first: it reads like a browser.

**Your servers stay yours.** No account and no cloud: hosts, keys and passwords stay on your computer, and passwords live in your system's keychain. SSHeesh talks to your servers (through Tor, if you like), and to GitHub to check for updates. Files you upload lose their metadata first (a photo's location and camera, a document's author); your own copy stays as it is.

**It's convenient from the first minute.** Paste a whole `ssh` command and every field fills in; your `~/.ssh/config`, keys and agent just work. Bring your servers over from SSHeesh 0.10, XPipe, PuTTY and other SSH apps when you add one. Errors come in plain words, and every command SSHeesh suggests says what it does.

<p align="center"><img src="docs/screenshots/1.0/welcome.png" alt="The welcome at a first start, page 1 of 4: Your servers, with rows to bring over a server from SSHeesh 0.10, pick from 2 in ~/.ssh/config, or add another by hand, and a note that nothing connects yet and everything stays on this computer"></p>

**You make it yours.** SSHeesh Default (a warm dark with a soft green), Daylight, Midnight, Graphite and OLED, each editable down to every color; any accent color and your own terminal colors; your own keyboard shortcuts; server icons in lines or pixels, in any color, or your own picture; and the welcome a server greets you with.

<p align="center"><img src="docs/screenshots/1.0/light.png" alt="The same window in the light Daylight theme: servers listed by name, a terminal and Files side by side"></p>

## Tricks worth knowing

- **Drag a tab onto the page** to see two at once, also two servers' tabs side by side. Ctrl+Shift+S (⇧⌘S) switches between one tab at a time and side by side.
- **Copy files from one server to another.** Drag them from one server's Files onto another's beside it, or right-click them and pick the server and the folder there. They go straight through SSHeesh, so the servers don't need to reach each other, and nothing is replaced without asking.
- **Open a server's file in your own editor.** Pick the editor in the file's menu; every save goes back to the server, and if the file changed there meanwhile, SSHeesh shows you before anything is overwritten.
- **A welcome that says where you are.** A new terminal shows the server's own welcome, or SSHeesh's: the system, uptime, memory and disk, containers and updates, beside the logo. The server's notices about updates always stay.
- **The main menu** (the logo at the top left) is a start page with all your servers, what's running and the tabs you closed. Ctrl+Shift+Z (⇧⌘T) brings back the last one.
- **One bar for everything.** Ctrl+Shift+L (⌘L) finds servers, folders, commands, snippets and settings. A command for a server says which server it runs on, and is never picked for you: you move to it and press Return.

<p align="center"><img src="docs/screenshots/1.0/command-bar.png" alt="The command bar open over a terminal, searching or running on web-7731: Files, Stats, Security check, Free up disk space, Web apps and tunnels, Recipes and Containers, then saved commands such as How full the disks are, some marked Prod"></p>

- **Your keys.** Every shortcut is listed in Settings → Keyboard, and you can change any of them.
- **Server icons that mean something.** SSHeesh sees what a server runs (a website, a database, Docker) and picks its icon; each server gets a color of its own. Right-click a server for 30 more icons in any color, or your own picture.
- **Streamer mode** (Ctrl+Shift+H, ⇧⌘H) hides server names, addresses and users everywhere, terminal output included, while you share your screen.
- **Everything about a server, in its tabs:** a quick file manager with bookmarks, uploads and downloads that shows pictures and Markdown where they are, its Docker containers grouped by Compose project with their logs and a shell, stats, web apps opened in your browser through your SSH login, tunnels, and step-by-step recipes for the usual jobs, whose steps go into the terminal for you to run.

<p align="center"><img src="docs/screenshots/1.0/files-picture.png" alt="Files on This computer in the Pictures folder: three pictures listed, and the one selected, a drawing of hills at dusk, shown large in the preview beside the list with its size, 1600 × 1000"></p>

<!-- 1.0: align with the release packaging before merging to main -->
## Install

| | |
| --- | --- |
| **macOS** | Download SSHeesh for Mac from the [Releases page](https://github.com/footprintsntrails/ssheesh/releases/latest) and open it. |
| **Windows** | Download SSHeesh for Windows from the [Releases page](https://github.com/footprintsntrails/ssheesh/releases/latest) and run it. |
| **Linux** | Download SSHeesh for Linux from the [Releases page](https://github.com/footprintsntrails/ssheesh/releases/latest). It draws with Vulkan, which Mesa's graphics drivers provide. |

SSHeesh updates itself, or only tells you, as Settings → Updates says. Builds aren't signed by Apple or Microsoft yet, so the first launch needs a few extra clicks.

**On your servers:** any Linux works. Install `tmux` for sessions that survive disconnects.

## Ideas and bugs

Feature requests are very welcome: [open one](https://github.com/footprintsntrails/ssheesh/issues/new?template=feature_request.yml). SSHeesh is built to one vision, so pull requests aren't accepted; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Build from source

You need [rustup](https://rustup.rs), which installs the Rust version named in `desktop/rust-toolchain.toml` on the first build. On Debian or Ubuntu, add these libraries and a Vulkan driver:

```bash
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libdbus-1-dev pkg-config libvulkan1 mesa-vulkan-drivers
```

Then:

```bash
sh scripts/vendor-rune.sh                 # Rune, the window library, with SSHeesh's fixes
cd desktop
cargo run -p ssheesh-preview              # run the app
cargo run -p ssheesh-preview -- --demo    # on made-up servers, connecting to nothing
```

| Path | What's there |
| --- | --- |
| `crates/sshee-core` | The core: ssh, `tmux`, files, Tor, metrics. No window. |
| `crates/sshee-app` | The app's rules without a screen: servers, tabs, settings, what each screen does. |
| `desktop/sshee-ui` | Everything drawn, on Rune: the window, the terminal, every screen. |
| `desktop/app` | The program itself, on your servers or on made-up ones (`--demo`). |

Everything remote runs through the system `ssh`, with connection sharing where it can (not on Windows), so files and stats reuse the terminal's login. Durable sessions are named `tmux` sessions on the server. With Tor on, ssh hands each connection to SSHeesh's own Tor client, [Arti](https://gitlab.torproject.org/tpo/core/arti), through a `ProxyCommand`. How the window is built: [desktop/ARCHITECTURE.md](desktop/ARCHITECTURE.md).

## Roadmap

What's planned next. The order may still change.

- **Docker and services.** Per-container stats, and updates you can roll back. systemd services, and version history for config files.
- **Workspaces.** A workspace per project, a drop-down terminal and a tray monitor. App lock, and `ssh://` links.
- **Sync.** Encrypted sync through a folder you pick, with no account.
- **Back from 0.10:** videos that play in Files, and a tab dragged out into a window of its own.
- **Along the way:** signed builds for macOS and Windows, and a built-in SSH connection on Windows, since its `ssh` can't share connections.

See what changed in each version in the [changelog](CHANGELOG.md).

## Credits

Made by [@footprintsntrails](https://github.com/footprintsntrails), built together with [Claude](https://claude.com/claude-code) by Anthropic.

The window is drawn by [Rune](https://github.com/anomalyco/rune) (MIT), with a few fixes of our own.

Terminal color schemes come from their open-source authors: Tokyo Night, Catppuccin, Dracula, Nord, One Dark, Gruvbox and Solarized.

Fonts are [JetBrains Mono](https://www.jetbrains.com/lp/mono/) and [Inter](https://rsms.me/inter/) (SIL Open Font License). Icons in prompts like oh-my-posh and Starship come from the [Nerd Fonts](https://www.nerdfonts.com) symbols font (MIT; each icon set under its own open license). Pixel server icons are [pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann (MIT). The full list is in [desktop/THIRD-PARTY.md](desktop/THIRD-PARTY.md).

Tor comes from [Arti](https://gitlab.torproject.org/tpo/core/arti), the Tor Project's Tor in Rust (MIT or Apache 2.0). SSHeesh isn't made or endorsed by the Tor Project.

## License

[CC BY-NC-SA 4.0](LICENSE). The code is open: you may use, study, share and modify SSHeesh for **non-commercial** purposes, as long as you credit the project and release anything you build on it under the same license, with its source open. Commercial use isn't allowed without permission.
