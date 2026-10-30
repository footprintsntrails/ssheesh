# Changelog

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
