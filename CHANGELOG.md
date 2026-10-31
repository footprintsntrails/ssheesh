# Changelog

## 0.5.1 — real tabs

- **Tabs you can move and close.** In the Tabs layout every terminal and file manager of the server is a tab: drag one to move it, close it with its × or a middle click, or right-click for "Close other tabs" and "Close tabs to the right". The + next to them opens a new terminal or file manager.
- **Ctrl+Tab and Ctrl+Shift+Tab** go to the next and previous tab (side by side: the next pane). **⌘T / Ctrl+Shift+T** opens a new terminal.
- **A tab keeps its name.** Closing "Terminal 2" no longer renames "Terminal 3"; the next new terminal takes the free number.
- **Stats stay at the bottom.** They're no longer a tab or a button at the top, and a server's workspace no longer opens a Stats pane: the vitals bar shows them, and clicking it opens the full view (click again to go back, in the Tabs layout).
- In the Tabs layout, a pane's title bar no longer offers to split or fill the window.

## 0.5.0 — commands and tunnels

### Snippets and a sharper command palette
- **Snippets: commands you keep and run again.** Leave blanks in them, like `{{service}}` or `{{lines:100}}`, and SSHeesh asks for them each time; `{{container}}` lets you pick one of the server's containers. They run in the terminal of the server you're looking at. A few useful ones are there to start with.
- **Narrow the palette down** (⌘K / Ctrl+Shift+K) by typing `srv:` for servers, `cmd:` for snippets, recipes and server tools, `file:` for bookmarked and recent folders, or `set:` for settings.

### Recipes that explain every command
- **Eight step-by-step guides**: log in with a key and turn passwords off, turn on the firewall, stop password guessing with fail2ban, install security updates automatically, use your own user instead of root, install Docker, add swap, and put a web app behind HTTPS with Caddy.
- **Every command comes with a sentence saying what it does.** Commands go into the terminal without pressing Enter, so you run each one yourself. Steps that could lock you out say so first, check that you can still log in, and say how to undo them.
- Commands for Debian and Ubuntu or for Fedora and RHEL, picked from what the server runs. Findings in the security check-up link to the recipe that fixes them.

### The server's web apps in your browser, and saved tunnels
- **Web apps on this server**: SSHeesh finds what the server serves (Grafana, a dev server, a Docker app) with its page title. Click Open and it's in your browser at `localhost`, through your SSH login, without opening anything to the internet.
- **Saved tunnels** bring a server's port (or a SOCKS proxy) to this computer, with a switch for each. They reconnect on their own when the connection drops, and the ones that were on start again when SSHeesh opens.

### Terminal
- **Ctrl+click a file path in the output** (⌘-click on a Mac) to open it in the file manager beside the terminal, with the file picked out and shown. Paths like `src/main.rs:12` and `./notes.md` work too, from the folder the terminal is in. Ctrl+click a web link to open it in your browser.
- **A scrollbar**, also for the history of server terminals, which lives in tmux on the server. Drag it, click above or below it, or press Shift+Page Up and Shift+Page Down. Typing while you're scrolled back takes you back to the prompt.
- **The server's welcome message** (motd) shows when a new session starts, as it does when you log in with ssh.
- **Resizing no longer makes the text jump.** The terminal takes its new size once you stop dragging, instead of redrawing on the server at every step.
- On Windows, connecting no longer types `61;6;7;21;22;23;24;28;32;42c` into the shell.

### Tabs or side by side
- **Terminal, Files and Stats switch on and off** at the top: with only Terminal on, the terminal fills the window; with Terminal and Files on, they share it. Switching one off keeps it running.
- **Or use them as tabs**: one fills the window at a time, and a second terminal gets a tab of its own. Pick it in the welcome or in Settings → Layout.

### File manager
- **Folders open much faster on Windows**, and big folders faster everywhere. SSHeesh stays signed in to the server between clicks instead of logging in again for every folder (Windows can't share one connection the way macOS and Linux do). Folders you've seen show up at once while they refresh, the folder under the pointer loads before you click it, and a folder with thousands of files only draws what's on screen.
- **Click any folder in the address bar** to go there; click the current folder or the space after it to type a path.
- **A `..` row** at the top of every folder goes up one.
- **Back and Forward** with the mouse's side buttons, Alt+← and Alt+→, and a Forward button.
- **Bookmarks on the left or the right**: Settings → Layout.

### Fixed
- "What's using the disk" promised far more from removing unused Docker images than it frees, and kept promising it after you'd removed them: Docker counts layers those images share with ones in use. It now counts only what the cleanup really removes, and leaves the card out when no image is unused.
- Drop-down lists on Windows showed white text on a white background.
- Picking an icon for a new server opens the icons under its name instead of a pop-up over the colors.

## 0.4.2 — the portable app updates itself

- **The portable Windows app updates itself**, like the installed one: it downloads the new version, checks its signature, takes its place, and asks you to restart. Your servers and settings stay put. This works from 0.4.2 on, so getting 0.4.2 itself is one last manual download.
- The security check-up and "What's using the disk" windows have a close button.

## 0.4.0 — know your server

### A security check-up
- **One click checks a server's security** without changing anything: password login, root login, ports open to the internet, the firewall, Docker ports that slip past ufw, automatic updates, protection against password guessing, and updates waiting for a restart.
- **Packages with known holes.** The check-up lists installed packages that have security fixes waiting, with their CVE numbers when the system knows them, and flags ssh, OpenSSL, sudo and the kernel. It asks the server's own package manager, so your package list goes nowhere.
- **Each finding says why it matters** in plain words, problems first. Where there's a fix, it's a command you can copy or paste into the server's terminal, and nothing runs until you press Enter.
- Find it in Stats, by right-clicking a server, or by searching for "security" (⌘K / Ctrl+Shift+K).

### What's using the disk
- **How full each disk is, and the biggest folders.** Click a folder to look inside, or open it in Files.
- **Docker's images, containers, volumes and build cache**, plus the system journal and package caches.
- **Safe cleanups** that say what they remove and roughly how much they free, as commands you copy or paste into the terminal. SSHeesh never deletes anything on its own, and doesn't offer to delete Docker volumes, which hold data.

### Start an AI agent where you are
- **The logos of the agents a server has** (Claude Code, Codex, OpenCode, oh-my-pi) sit in the terminal's title bar. Click one to start it in the folder you're in. If the terminal is busy with something else, the agent starts in a new terminal beside it, in the same folder.

### Terminals made for AI coding agents
- **Claude Code, OpenCode and other full-screen programs get full color.** Servers now know your terminal shows millions of colors, so programs stop falling back to 256.
- **Esc works right away**, also on servers with an older tmux, which waited half a second for more keys. Stopping an agent feels instant.
- **Shift+Enter starts a new line** in Claude Code, OpenCode and Codex instead of sending your message. In a plain shell it still runs the command, like Enter.
- **Icons in oh-my-posh and Starship prompts show up** instead of empty boxes: SSHeesh brings the Nerd Fonts symbols along.
- **Emoji take two cells**, as the server counts them, so the cursor no longer drifts after one.
- **Draw with the GPU** in Settings → Terminal: smoother for programs that redraw a lot, and box lines join up. On by default on Windows.

### Copy and paste
- **Selecting text copies it**, in server terminals too. So do the copy commands of Claude Code, OpenCode and vim. Turn it off in Settings → Terminal.
- **Ctrl+C copies when text is selected**, as in Windows Terminal; with nothing selected it stops the running command, as always. Ctrl+Shift+C and Ctrl+Shift+V always copy and paste.
- **Pasting on Windows.** Ctrl+V pastes, and so does clipboard history (Win+V), which presses Ctrl+V for you. Before, it sent a control key to the server instead.
- A server can put text on your clipboard but never read it.

### See uploads and downloads
- **A card in the corner for every upload and download**: how much is done out of how much, the speed, and an X to stop it. Stopping leaves nothing half-done behind, on the server or on your computer.
- An upload that gets cut off no longer leaves a broken file in place of the one that was there.
- The drop-to-upload window closes as soon as you press Upload; the card shows how it goes.

### Fixed
- A file preview no longer spills out of a narrow Files pane: it takes the whole pane until there's room for both. The wide bookmarks column waits for room too, and the status bar stays on one line.
- The vitals bar keeps its buttons in a narrow window, and toasts sit above it instead of covering it.
- Settings' close button stays put while you scroll.
- In compact mode the toolbar waits a moment before sliding in, so reaching for a pane's buttons doesn't cover them. Panels that slide in have an edge line.
- A server's card in the icon rail no longer says it isn't open when Files or Stats is.
- The Compact mode switch in the welcome no longer gets squeezed by its description, and the welcome's choice buttons don't stretch across.
- The Add server window shows that there's more below.

## 0.3.5 — live servers

### See how your servers are doing
- **Small CPU and memory meters under every open server** in the sidebar. They come from the same numbers as the vitals bar, and servers you haven't opened aren't asked for anything.
- **A warning line when a disk or memory is nearly full**, like "Disk 91% full", so you notice before something breaks.
- **When the sidebar is folded into icons**, each open server shows its load on its icon. Pick the style in Settings → Layout → Server load, or in the welcome's Layout step: Frame (the default), Bars, Pulse, Count, or Off.
- **Point at a server icon** for a card with its address, how many panes are open, CPU, memory, disk and uptime.

### Fixed
- **"Remember password" works after a restart.** The password went into your keychain, but SSHeesh forgot it had saved it and asked again next time. Servers you added before this update find their saved password on the next connect, so there's no need to add them again.

## 0.3.4 — drop files anywhere

- **Drop files on a server's terminal to upload them.** SSHeesh asks where to put them, starting with the folder that terminal is in, and you can change it before uploading.
- **Drop files on a file manager** and they go straight into the folder it has open, on a server or on this computer.
- Dropping files on this computer's terminal types their paths, like any terminal app.
- While you drag, the pane under your files lights up and says what will happen.
- The **Root** place is now called **Disk**, so it doesn't read like the root user's folder. Home's tooltip shows its real path (`/root` when you sign in as root).
- ssh's notes about its shared connection ("Shared connection to … closed.") no longer show up in terminals or error messages.

## 0.3.3 — set it up your way

### A guided setup
- **The welcome walks you through making SSHeesh yours.** Colors and accent; the terminal's color scheme, font, text size and cursor; the layout (server list as names or icons, which side the sidebar sits on, the bookmarks column, vitals, compact mode); and how it behaves (reconnecting, "Needs you" alerts, highlighting, the logo). A live preview of the app shows every choice as you make it. Skip any step, or the whole thing.

### What's new, right in the app
- **After an update, SSHeesh shows what changed**, including every version you skipped. See it again any time in Settings → About, or search for "What's new".

## 0.3.2 — friendlier and tidier

### A proper hello
- **A short welcome on first launch.** Pick a theme and an accent, choose which servers from `~/.ssh/config` to bring along, and three tips worth knowing. Every step can be skipped, and everything in it can be changed later in Settings.

### Fits any size
- **Pane title bars keep their Close button**, however many panes you open. In narrow panes the other buttons fold into a ⋯ menu, and labels shorten before buttons disappear.

### A tidier file manager
- **Take places out of the column.** Hold a place, bookmark or recent folder and drag it out of the column to remove it. Right-click the column to tick which places show, or bring them all back.
- **A New menu** (the + button): a new file, a new folder, or an archive of what you selected (or of everything here) as a `.tar.gz`. Right-click any file or folder to archive it too.
- **A toolbar that fits.** In a narrow file manager the address bar keeps its room; the filter, bookmark, upload and refresh buttons step aside into the ⋯ menu, which always has them all.
- "Show hidden files" moved from the toolbar to the ⋯ menu and the right-click menu.

### Also
- **The logo moved to the bottom of the sidebar**, bigger, with the version next to it. On a Mac it no longer sits under the window buttons. Click it for About.
- **A calmer top bar.** The layouts and vitals buttons are gone: layouts live in Settings and the command palette, and the vitals bar has its own close button (⌘J or Ctrl+Shift+J brings it back).

## 0.3.0 — make it yours

A big one. SSHeesh is about a terminal you can shape to the way you work, and this release adds a lot of that, plus a much friendlier way to add servers, and guard rails for the servers that matter.

### Make it yours
- **Server icons and colors.** Pick an icon and a color for every server, when you add it or with a right-click.
- **A narrow sidebar.** Drag the sidebar's edge in and the server list folds into a rail of icons. Drag it out again for names.
- **Compact mode** (⇧⌘M or Ctrl+Shift+M). The sidebar, toolbar and vitals step aside; point at the window's edge and they slide back in.
- **Light by day, dark by night.** Let the theme follow your system, with a light and a dark theme of your choice.
- **Interface size**, separate from the terminal's font size.
- **Fade the panes you're not using**, so it's obvious where you're typing.
- **Search in Settings.** Type "font" or "reconnect" and jump straight to it.
- **A logo that winks.** The `>_<` mark now sits in the sidebar and on empty panes, and blinks at you now and then. Prefer it still, or gone? Settings → Appearance.
- Flip through themes with the arrow keys, and turn animations off if you like things calm.

### Easier every day
- **A new Add a server window.** Paste anything: `root@1.2.3.4:2222` or a whole `ssh -p 2222 -i key user@host` command, and the fields fill in. Your keys in `~/.ssh` and key managers like 1Password and Bitwarden are listed for you.
- **Test before you add.** One button checks that the server answers, shows its fingerprint the first time, signs in, and tells you whether `tmux` is installed (with the command to install it if not).
- **Passwords.** Sign in with a password, and choose whether SSHeesh remembers it. Remembered passwords live in your system keychain (Keychain, Credential Manager or Secret Service), never in SSHeesh's own files.
- **Highlighting in terminal output.** Errors in red, warnings in yellow, and your own words or regular expressions in the color you pick.
- **"Needs you" alerts.** When an AI coding agent or any program in a terminal waits for an answer, or a long run goes quiet, the pane and the sidebar say so, and you get a system notification if SSHeesh is in the background.
- **While you were away.** After a dropped connection, a card shows what the server printed in the meantime, with errors counted and marked.
- **You choose which ~/.ssh/config servers to add.** On first launch SSHeesh asks instead of adding all of them, and new ones later get a quiet note. Change your mind in Settings → Layout.
- **Remove or hide servers.** Right-click a server you added to remove it, or one from `~/.ssh/config` to hide it.
- **A portable Windows app.** One `.exe` to run without installing. Your servers and settings stay in `%APPDATA%` either way.
- **The window comes back the way you left it**, size and place, and folded panels unfold to the width they had.
- **Open in main terminal.** Right-click a folder in the file manager to `cd` the terminal you're already using there.
- New terminals opened next to files or stats go on the left.
- The bookmarks column in the file manager is narrow by default and looks cleaner.
- Empty spots explain what to do next, like how to install Docker on a server that doesn't have it.

### Safer
- **Prod, Staging and Dev labels.** A colored tag and frame, everywhere the server shows up.
- **A clear question for new servers.** The first connection shows the server's fingerprint in a card instead of ssh's yes/no prompt. If a server's identity ever changes, a red warning shows the old and new fingerprints and doesn't connect unless you say so.
- **Streamer mode** (⇧⌘H or Ctrl+Shift+H). Hides server names, addresses, IPs and user names everywhere, terminal output included, for streams and screen shares.
- **Privacy page** in Settings: everything SSHeesh connects to, which is your servers and GitHub for updates. No account, no analytics.
- When the connection drops, SSHeesh now asks before reconnecting by default. Settings → Terminal switches it back to automatic.

### Coming next
- 0.3.2 tidies up the file manager and adds a first-launch welcome. See the [roadmap](README.md#roadmap) for what follows.

### Fixes
- The sidebar and the bookmarks column can be dragged narrower again; the drag used to get stuck.
- The macOS install guide covers macOS 15 and newer, where right-click → Open no longer works, and there's a one-line Terminal installer that skips the security prompt.

## 0.1.0 — first preview

The first public build. It's early and rough: expect bugs, and please [report them](https://github.com/footprintsntrails/ssheesh/issues/new/choose).

### Terminals
- Sessions survive network drops. On servers with `tmux`, reconnecting lands you back in the same shell with everything still running.
- Choose what happens when the connection drops: reconnect automatically, show a Reconnect button, or stay quiet.
- The status face tells you how a session is doing: `>_` connected, `>_<` reconnecting, `x_x` couldn't connect.
- Nine color schemes, font, size, line height, cursor and padding.

### Containers
- Expand a server in the sidebar to see its Docker and Podman containers, grouped by Compose project.
- Open a shell inside a container, follow its logs, start, stop or restart it.

### Files
- Browse a server's files with an address bar you can type into, places, bookmarks and recent folders.
- Preview and edit text files, upload by dragging files in, download files and whole folders.

### Stats
- A one-line vitals bar (CPU, memory, network, disk) and a detailed view with load, traffic and every disk. Nothing to install on the server.

### Workspace
- Split panes, drag them around by their title bar, resize any edge, fill the window with one pane.
- Command palette, keyboard shortcuts, sidebar on either side.
- Four app themes, accent colors, spacing and corner settings.
- Updates install themselves; you choose automatic, ask first, or never.
