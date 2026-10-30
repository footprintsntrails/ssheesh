# Changelog

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
- 0.3.1 tidies up the file manager: drag places and bookmarks out to remove them, a New menu for files, folders and archives, and a toolbar that adapts to narrow panes. See the [roadmap](README.md#roadmap) for what follows.

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
