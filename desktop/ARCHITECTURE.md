# SSHeesh 1.0 on Rune: how the window is built

For whoever builds the next screen. `desktop/` is 1.0's own Cargo workspace (Rune and its
patched crates stay out of the 0.10 app in the root workspace); build and test from inside it.

## The crates

| Crate | Rune? | What it holds |
|---|---|---|
| `crates/sshee-core` (root workspace) | no | ssh, tmux, files, Tor, metrics: everything that talks to a machine |
| `crates/sshee-term` (root) | no | the terminal without a screen: engine, keys, secrets, selection |
| `crates/sshee-app` (root) | no | the app's state without screens. `workbench` is what the window shows and does: servers, the row of tabs, the tab on screen, shortcuts, vital signs, saved tabs. `model`, `import`, `legacy` are 0.10's stores and bringing them over |
| `desktop/sshee-ui` | **yes** | everything drawn: `theme` (tokens), `icons`, `kit` (components), `terminal` (the pane), `shell` (the window), `screens` (one module per screen). It doesn't depend on sshee-core |
| `desktop/app` (`ssheesh-preview`) | through sshee-ui | `main`: which copy this is (`AppIdentity::preview`), ssh's helper roles (Tor proxy, askpass), and the window's backend: `live.rs` on sshee-core, `demo.rs` on made-up data |
| `desktop/proto` (`ssheesh-proto`) | through sshee-ui | the terminal prototype: one window, one pane, the meter |

Rule: Rune is named only in `desktop/sshee-ui`. A rule that can be decided without a window
(which tab comes up when one closes, what a shortcut is, how a number reads) goes in
`crates/sshee-app` with unit tests, not in a view.

## How state flows

```text
                         UI thread (Rune)                                other threads
 click, key ─▶ handler(&mut Shell, &mut Tx)                    session-read: model.feed(bytes) ─▶ waker
                 │  s.bench.<rule>()      sshee-app, pure            │            (once per frame)
                 │  s.settle(tx)          panes, keyboard, watch     │
                 ▼                                                   ▼
 frame start:  AppSpec::poll ─▶ Shell::poll ─▶ every TermPane::poll (frames, replies, bell)
                 ▼
               view(&Shell, &mut Cx) -> El      reads s.bench, s.panes, s.screens; builds elements
                 ▲
 Remote::update(|s: &mut Shell| ..) ◀── status sinks, vitals thread, labels thread, Tor's watcher
```

- **Who owns it.** One `sshee_ui::shell::Shell` per window (`shell/state.rs`): the
  `Workbench` (`s.bench`), the terminal panes by tab (`s.panes`), each screen's own state
  (`s.screens`), and the `Backend`. Rune owns the `Shell` and hands it to handlers.
- **How a view reads it.** `fn view(s: &Shell, cx: &mut Cx<Shell>) -> El` runs again after
  every handler and every `poll` that returns true. It only reads. Hover, press and the focus
  ring are paint (`El::hover`, `El::press`, `Cx::focus_ring`): they never run the view.
- **How an action changes it.** A handler calls one method of `Shell` (`go_to`, `new_terminal`,
  `focus_tab`, `close_tab`, ...). Each applies a `Workbench` rule and ends with
  `Shell::settle(tx)`: panes of closed tabs end, the tab on screen gets its pane and the
  keyboard, the backend hears which machine to watch and which tabs to save. A new action is a
  new method that ends the same way.
- **How background threads wake the UI.** Two ways, by how often they speak:
  - *Terminal output* (thousands of reads a second): the session's reader thread parses into the
    pane's `sshee_term::Model` and calls a coalescing waker; `Shell::poll` pulls at most one
    frame per pane per display frame. Never `Remote` for output.
  - *Everything else* (a session's status, vital signs, Tor, the server list): the thread holds a
    `Remote` (cloneable, `Send`) and calls `remote.update(|s| ..)`; the closure runs on the UI
    thread, in order, and the view runs after it. `remote.quietly(..)` runs without drawing. A
    `Backend` gets its `Remote` in `Backend::start`. Never block the UI thread: anything that
    waits (ssh, the keychain, a file listing) runs on a thread and comes back through `Remote`.
- **Where sessions come from.** The window asks its `Backend` (`shell/state.rs`) for a
  `TermLink` per terminal tab. `desktop/app/src/live.rs` gives sshee-core sessions;
  `demo.rs` and the tests give fixtures. A screen that needs a machine (files, stats, docker)
  adds what it needs to `Backend` as methods with a default body, implements them in `live.rs`
  and with made-up data in `demo.rs`, and returns results through `Remote`.

## A terminal pane and what sits over it

`terminal/view.rs` draws the grid, the cursor and the input method; `terminal/overlays.rs`
wraps it (`term-box`) with what sits over the text, each in its own file with its state in
`TermPane::over`:

| File | What | Where its rule lives |
|---|---|---|
| `find.rs` | the find bar; the search of the whole history (tmux's over ssh, else the grid) off the UI thread | `sshee_term::find`, marks by the presentation pass |
| `links.rs` | the link under the pointer (underline, hint), Ctrl/⌘-click | `sshee_term::links`, `present::Link` |
| `scrollbar.rs` | the scroll bar, Shift+Page Up/Down, "Jump to the bottom" | `sshee_term::scroll` |
| `away.rs` | the "While you were away" line between two rows of the grid | `sshee_term::away` |
| `menu.rs` | the pane's menu on the secondary button | `kit::menu` |

- **Hidden things stay hidden.** Find searches rows made safe by `Look::safe_rows`, on the
  thread that read them; marks, links and their hints come from the presentation pass. Nothing
  here reads the engine's text to show it.
- **Slow things come back through the pane's `Postbox`** (`overlays.rs`): a thread posts a
  `Note` and wakes the window; `TermPane::poll_overlays` takes it. A replayed session does the
  same work at once, so headless frames don't depend on a thread. What needs the server goes
  through `TermLink::ask` (`Ask::Rows`, `History`, `Scroll`), which `live.rs` answers from the
  shell's tmux session.
- **What only the window can do is a `Wish`** the window takes after each poll
  (`Shell::grant` in `shell/tabs.rs`): a new terminal here, a file's path to open
  (`Shell::open_path`: the seam the Files screen takes; until then the path is copied), the
  shell's folder (the tab's name, `Shell::set_folder`), a press on a pane shown beside another.
- **Streamer mode** is `shell/streamer.rs`: `Shell::server_shown(key, name)` and
  `Shell::text_shown(text)` are how every screen draws a server's name or any text that may
  hold one; the terminals follow `Shell::look()`.
- **Copying**: `PaneOptions::copy_on_select` and `program_clipboard` (a program's OSC 52), set
  for every pane by `Shell::set_copy_modes`. A program that has the mouse gets the drag;
  Shift makes it the terminal's own selection.

## Tokens, icons, components

- `sshee-ui/src/theme.rs`: every colour (both appearances, from the design's stylesheet), size,
  radius and text style. `Theme::of(cx.env().system_dark)`. No colour or size is written
  anywhere else. The design: `research_notes/v1-plan/fresh-apple.html` (local notes).
- `sshee-ui/src/icons.rs`: `Icon` and `icon(Icon, size, colour)`; a new icon is one arm of
  `Icon::drawing` with SVG path data (`svg.rs` reads it), on the 16-unit grid, 1.4 pt line.
- `sshee-ui/src/kit/`: one module per component (`button`, `tab`, `rail`, `chip`, `menu`,
  `popover`, `status`, and for forms and lists `field`, `list`, `tile`, `push`). A component is a function from what it shows to an `El`; the caller
  chains `.key(..)` and handlers. Add states to the component, not at the call site.
- From Rune itself, used as is: tooltips (`El::help`), focus rings and Tab order, text fields
  (`rune::text_input`), native pop-up menus, virtual lists (`El::virtual_rows`). **rune-kit**
  (menus, popovers, dialogs, tables, trees, Markdown, in OpenCode's look) is not a dependency:
  it pulls rune-highlights (a WASM-to-Rust build) and its own theme. The first screen that
  needs a big piece of it (a table for Files, Markdown for previews) should add it as a git
  dependency at Rune's pinned rev and wrap it in `kit/`; small things are cheaper to draw here.

## Adding a screen

1. **A tab's page** (Files, Stats): `sshee-ui/src/screens/<name>.rs` already has `State` and
   `view(s, tab, t, cx)`; `shell/view.rs::page` calls it. Keep the screen's state in its
   `State` (`s.screens.files[&tab.id]`), its rules in a new module of `crates/sshee-app`.
2. **A new tab kind**: a variant of `workbench::TabKind` (label, 0.10's kind), its icon in
   `shell/view.rs::tab_icon`, its page in `page`, its module in `screens/`.
3. **Something over the window** (the command palette, settings, an agent's question, a
   server's card): a module in `screens/` with its `State` in `screens::States`, drawn as a
   last child of the root (Rune's `El::modal` traps focus and closes on a click outside),
   opened by a `Shell` method. Its element goes in `screens::overlays`, its "is it open" in
   `States::any_open` (no shortcut fires and no key reaches a terminal behind it) and its
   closing in `States::close_all` (one thing over the window at a time). Use `kit::popover`,
   `kit::list` and `kit::push`; the palette (`screens/palette.rs`) is the one to copy for a
   list with one moving highlight, "Add a server" for a form.
4. Keys are fixed words and tab numbers (`format!("tab-{}", id.0)`), never a server's name.
5. Tests: rules in sshee-app; the screen in `Runtime::with_text` (see `shell/tests.rs`: a fake
   `Backend`, clicks by element key, the screen reader's tree); one headless picture in
   `desktop/app/tests/window.rs` when the look matters. Add the screen's made-up data to
   `desktop/app/src/demo.rs`.

## Running it and taking pictures

```sh
cd desktop
cargo run -p ssheesh-preview -- --demo            # made-up servers, nothing connects
cargo run -p ssheesh-preview -- --demo --sheet    # every component in its states
cargo run -p ssheesh-preview                      # your servers (0.10's list, read only)
cargo test --workspace && cargo clippy --workspace --all-targets
# a picture without a window (needs a Vulkan driver; on Linux without a GPU, lavapipe):
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json ../target/desktop/debug/ssheesh-preview \
  --demo --theme light --size 1280x800 --scale 2 --frames 20 --snap shot.png --tree tree.json
# with input: --script events.json --at mark:end   (see app/tests/window.rs)
```

Preview keeps its own files in `~/.sshee-preview` (shared connections, Tor, `tabs.json`) and
never writes 0.10's. `--demo` reads and writes nothing.

## Known gaps in the shell (not screens)

- Linux keeps the system's title bar above the window: Rune draws no frame there, and a
  borderless window can't be resized by its edges (Rune doesn't expose winit's
  `drag_resize_window`). macOS and Windows get the design's bar (`App::titlebar`); neither has
  been run yet, nor have their system fonts (`fonts::SYSTEM_FAMILIES`).
- The row of tabs (`shell/tabs.rs`) has its menu, drag to reorder and a list for tabs out of
  sight; a tab can't be dragged out into a window of its own, and side by side
  (`shell/split.rs`, `workbench::layout`) is one pair of one machine's tabs, not 0.10's tree of
  splits. Pairs aren't kept between runs.
- A terminal tab is named after its shell's folder when the shell says it (OSC 7, or a
  `user@host: ~/dir` title). In tmux on a server neither reaches the window (tmux keeps both):
  the agents' watch reads `pane_current_path` and should call `Shell::set_folder`, and
  `TermPane::set_foreground` with the program in front. The program in front is wired
  (`app/src/agents.rs` asks tmux, `screens::agent::foreground` names the tab "Terminal N
  (Claude Code)" and tells the pane); the folder isn't yet.
- A system notification's text is made and handed to `Backend::notify`, which shows nothing
  yet: the notification itself, and its click bringing the tab forward, are the desktop
  crate's step.
- A click on a file's path copies it: `Shell::open_path` waits for the Files screen. A web
  link whose text hides where it goes opens on a second Ctrl+click, not from a dialog.
- Streamer mode doesn't know this computer's hostname (only its user, from the environment):
  `Backend` has nothing to ask yet. Whether it is on isn't kept between runs (Settings will).
- Saved passwords aren't read (0.10's keychain needs the owner's yes); a server that asks for
  one asks in the terminal. Host-key questions have no screen.
- Tor's round-trip time isn't measured; the status bar shows "Tor" without it.
- Every hidden pane still takes its frames in `poll` (so a switch shows it at once); with
  dozens of busy tabs that wants a "drain replies only" mode in `TermPane`.

## Screens that are built

| Screen | In `sshee-ui/src/` | Rules in `crates/sshee-app/src/` |
|---|---|---|
| Main menu: search field, server tiles, agents at work, closed tabs | `screens/home.rs` | `home.rs` |
| Command palette (`srv:` `cmd:` `file:` `set:`; a command for a server is never preselected) | `screens/palette.rs` | `palette.rs` |
| Add a server: by hand, from the ssh config, XPipe and 0.10 | `screens/add_server.rs` | `servers.rs` (`Form`, `Ask`, `Answer`, and `Book`, the list in memory a demo and the tests answer from) |
| A server's menu (rename, icon and colour, label, remove) and its card under the pointer | `screens/server_menu.rs`, `screens/server_card.rs` | `labels.rs`, `home.rs` (`Card`) |

The list of servers is this copy's own: `desktop/app/src/servers.rs` keeps `hosts.json` and
`looks.json` in `~/.sshee-preview` and passwords under the copy's keychain name. The window
never touches it: a screen calls `Backend::servers(Ask)`, the backend does the work on a
thread and answers through `Remote` (`Shell::servers_answer`, with the new list). 0.10's list
is only read, to offer "Bring over your servers from 0.10".

## Screens still to port, and the files each owns

Each row can be built by itself. "Owns" are files only that screen's author edits; the shared
seams are one line each in `shell/view.rs` (where it's drawn), `shell/state.rs` (the method
that opens it) and `screens.rs` (its `State`), plus `Backend` methods in `shell/state.rs`,
`app/src/live.rs` and `app/src/demo.rs`.

| Screen (0.10's source) | Owns in `sshee-ui/src/` | Owns in `crates/sshee-app/src/` |
|---|---|---|
| Files: folders, previews, editor, drag in and out (`FilesPane.tsx`, `FolderPeek`, `MediaPreview`, `MarkdownView`, `UploadDialog`, `CopyToServer`) | `screens/files.rs`, `screens/files/` | `files.rs`, `paths.rs`, `dir_cache.rs` |
| Transfers (`Transfers.tsx`) | `screens/transfers.rs` | `transfers.rs` |
| Stats page and the sidebar's load card (`StatsPane.tsx`, `ServerLoad.tsx`) | `screens/stats.rs` | `workbench/vitals.rs` grows history |
| Settings and its sections (`SettingsDialog.tsx`, `ShortcutsEditor`, `ThemeEditor`, `ColorPicker`) | `screens/settings.rs`, `screens/settings/` | `model/settings.rs` (exists), `settings_store.rs` |
| The wide sidebar and tabs under each server (`Sidebar.tsx`) | `shell/rail.rs` (split out of `view.rs`) | `workbench.rs` |
| Zoom, a tab dragged out to its own window, more than two side by side (`Workspace.tsx`, `Pane.tsx`); the tab menu, drag and the pair are built | `shell/tabs.rs`, `shell/split.rs` | `workbench/layout.rs` |
| Compact modes and sliding edges (`EdgeReveal.tsx`) | `shell/compact.rs` | `workbench.rs` |
| An agent's question and answers; agent names on tabs (`agents.ts`, the design's popover). Built: names and "Needs you" on tabs, the card under a waiting agent's tab, the list of agents (Ctrl+Shift+A); the question is read by `sshee-term::question`. Not built: the buttons that start an agent | `screens/agent.rs`, `screens/agent/` | `agents.rs`, `agents/answer.rs` |
| Host-key and password questions, reconnect and sleep notices (`TerminalPane.tsx` overlays) | `screens/prompts.rs` | `prompts.rs` |
| In the terminal, still to do: highlights (`Look::highlights`, drawn as `Role::Paint`), the recap's full list of lines, drop files to upload (find, links, away, the scroll bar, the menu and streamer mode are built) | `terminal/` | none (sshee-term has them) |
| Tor: the switch, progress, log (`TorButton.tsx`) | `screens/tor.rs` | `tor.rs` |
| Security check and fixes (`ServerTools.tsx`, `Recipes.tsx`) | `screens/checkup.rs`, `screens/recipes.rs` | `recipes.rs` |
| Docker and containers (`ServerTools.tsx`) | `screens/containers.rs` | `containers.rs` |
| Tunnels (`Tunnels.tsx`) | `screens/tunnels.rs` | `tunnels.rs` |
| Snippets (`Snippets.tsx`) | `screens/snippets.rs` | `snippets.rs` |
| Toasts and confirmations (`Overlays.tsx`) | `kit/toast.rs`, `kit/dialog.rs` | none |
| Welcome, tour, what's new, hints (`Welcome.tsx`, `Tour.tsx`, `WhatsNew.tsx`, `Hints.tsx`) | `screens/welcome.rs`, `screens/tour.rs`, `screens/whats_new.rs` | `tour.rs`, `changelog.rs` |
| Second window, the mini window (`windows.ts`, `MiniApp.tsx`) | `shell/windows.rs` | `windows.rs` |
| Notifications with a click, tray (outside the window) | none | a new Rune-free crate, `crates/sshee-desktop` |

Bringing settings and tabs over from 0.10 at first start (`sshee-app::import`) isn't wired.
Servers, with their labels and looks, come over when you pick them in "Add a server"
(`desktop/app/src/servers.rs`); snippets are still 0.10's starters (`screens::palette::State`).
