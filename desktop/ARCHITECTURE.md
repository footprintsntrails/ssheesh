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

## Tokens, icons, components

- `sshee-ui/src/theme.rs`: every colour (both appearances, from the design's stylesheet), size,
  radius and text style. `Theme::of(cx.env().system_dark)`. No colour or size is written
  anywhere else. The design: `research_notes/v1-plan/fresh-apple.html` (local notes).
- `sshee-ui/src/icons.rs`: `Icon` and `icon(Icon, size, colour)`; a new icon is one arm of
  `Icon::drawing` with SVG path data (`svg.rs` reads it), on the 16-unit grid, 1.4 pt line.
- `sshee-ui/src/kit/`: one module per component (`button`, `tab`, `rail`, `chip`, `menu`,
  `popover`, `status`). A component is a function from what it shows to an `El`; the caller
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
   last child of the root in `shell/view.rs::view` (Rune's `El::modal` traps focus and closes
   on a click outside), opened by a `Shell` method. Use `kit::popover` and `kit::menu`.
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
- The row of tabs has no drag to reorder (`Workbench::move_tab` exists), no context menu, and
  when crowded it shrinks tabs and names each server once, but doesn't scroll.
- A terminal tab's title is "Terminal N", or "Terminal N (Claude Code)" while an agent is in
  front (`sshee_app::agents`, asked from tmux by `app/src/agents.rs`); the working folder
  (OSC 7) isn't a title yet (`Workbench::set_title` exists; the pane keeps it, `TermPane::cwd`).
- A system notification's text is made and handed to `Backend::notify`, which shows nothing
  yet: the notification itself, and its click bringing the tab forward, are the desktop
  crate's step.
- Saved passwords aren't read (0.10's keychain needs the owner's yes); a server that asks for
  one asks in the terminal. Host-key questions have no screen.
- Tor's round-trip time isn't measured; the status bar shows "Tor" without it.
- Every hidden pane still takes its frames in `poll` (so a switch shows it at once); with
  dozens of busy tabs that wants a "drain replies only" mode in `TermPane`.

## Screens still to port, and the files each owns

Each row can be built by itself. "Owns" are files only that screen's author edits; the shared
seams are one line each in `shell/view.rs` (where it's drawn), `shell/state.rs` (the method
that opens it) and `screens.rs` (its `State`), plus `Backend` methods in `shell/state.rs`,
`app/src/live.rs` and `app/src/demo.rs`.

| Screen (0.10's source) | Owns in `sshee-ui/src/` | Owns in `crates/sshee-app/src/` |
|---|---|---|
| Main menu: servers, running now, "Where to?" (`Home.tsx`) | `screens/home.rs` | `home.rs` |
| Files: folders, previews, editor, drag in and out (`FilesPane.tsx`, `FolderPeek`, `MediaPreview`, `MarkdownView`, `UploadDialog`, `CopyToServer`) | `screens/files.rs`, `screens/files/` | `files.rs`, `paths.rs`, `dir_cache.rs` |
| Transfers (`Transfers.tsx`) | `screens/transfers.rs` | `transfers.rs` |
| Stats page and the sidebar's load card (`StatsPane.tsx`, `ServerLoad.tsx`) | `screens/stats.rs` | `workbench/vitals.rs` grows history |
| Command palette, search (`Overlays.tsx` palette, `cmdk`) | `screens/palette.rs` | `palette.rs` |
| Settings and its sections (`SettingsDialog.tsx`, `ShortcutsEditor`, `ThemeEditor`, `ColorPicker`) | `screens/settings.rs`, `screens/settings/` | `model/settings.rs` (exists), `settings_store.rs` |
| Add a server, import from ssh config and XPipe (`AddServerDialog.tsx`, `ConfigImport`, `XpipeImport`) | `screens/add_server.rs` | `servers.rs` |
| A server's card and menu, labels and looks (`Sidebar.tsx` menus, `LookPicker`, `HostIcon`) | `screens/server_card.rs`, `screens/server_menu.rs`; more arms in `icons.rs` | `labels.rs` |
| The wide sidebar and tabs under each server (`Sidebar.tsx`) | `shell/rail.rs` (split out of `view.rs`) | `workbench.rs` |
| Tab menu, drag to reorder, side by side, zoom (`Tabs.tsx`, `Workspace.tsx`, `Pane.tsx`) | `shell/tabs.rs` (split out of `view.rs`), `shell/split.rs` | `workbench/layout.rs` (port of `layout.ts`) |
| Compact modes and sliding edges (`EdgeReveal.tsx`) | `shell/compact.rs` | `workbench.rs` |
| An agent's question and answers; agent names on tabs (`agents.ts`, the design's popover). Built: names and "Needs you" on tabs, the card under a waiting agent's tab, the list of agents (Ctrl+Shift+A); the question is read by `sshee-term::question`. Not built: the buttons that start an agent | `screens/agent.rs`, `screens/agent/` | `agents.rs`, `agents/answer.rs` |
| Host-key and password questions, reconnect and sleep notices (`TerminalPane.tsx` overlays) | `screens/prompts.rs` | `prompts.rs` |
| In the terminal: find, links, "while you were away", streamer mode, scroll bar, context menu (`termFind.ts`, `terminalLinks.ts`, `away.ts`) | `terminal/find.rs`, `terminal/links.rs`, `terminal/away.rs`, `terminal/menu.rs` | none (sshee-term has them) |
| Tor: the switch, progress, log (`TorButton.tsx`) | `screens/tor.rs` | `tor.rs` |
| Security check and fixes (`ServerTools.tsx`, `Recipes.tsx`) | `screens/checkup.rs`, `screens/recipes.rs` | `recipes.rs` |
| Docker and containers (`ServerTools.tsx`) | `screens/containers.rs` | `containers.rs` |
| Tunnels (`Tunnels.tsx`) | `screens/tunnels.rs` | `tunnels.rs` |
| Snippets (`Snippets.tsx`) | `screens/snippets.rs` | `snippets.rs` |
| Toasts and confirmations (`Overlays.tsx`) | `kit/toast.rs`, `kit/dialog.rs` | none |
| Welcome, tour, what's new, hints (`Welcome.tsx`, `Tour.tsx`, `WhatsNew.tsx`, `Hints.tsx`) | `screens/welcome.rs`, `screens/tour.rs`, `screens/whats_new.rs` | `tour.rs`, `changelog.rs` |
| Second window, the mini window (`windows.ts`, `MiniApp.tsx`) | `shell/windows.rs` | `windows.rs` |
| Notifications with a click, tray (outside the window) | none | a new Rune-free crate, `crates/sshee-desktop` |

Bringing settings and tabs over from 0.10 at first start (`sshee-app::import`) is wired only
for labels and looks so far (`desktop/app/src/servers.rs`).
