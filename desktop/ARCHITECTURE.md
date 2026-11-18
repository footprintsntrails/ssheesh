# SSHeesh 1.0 on Rune: how the window is built

For whoever builds the next screen. `desktop/` is 1.0's own Cargo workspace (Rune and its
patched crates stay out of the 0.10 app in the root workspace); build and test from inside it.

## The crates

| Crate | Rune? | What it holds |
|---|---|---|
| `crates/sshee-core` (root workspace) | no | ssh, tmux, files, Tor, metrics: everything that talks to a machine |
| `crates/sshee-term` (root) | no | the terminal without a screen: engine, keys, secrets, selection |
| `crates/sshee-app` (root) | no | the app's state without screens. `workbench` is what the window shows and does: servers, the row of tabs, the tab on screen, shortcuts, vital signs, saved tabs. `model`, `import`, `legacy` are 0.10's stores and bringing them over. `settings_store` is a running 1.0's settings: the file, the catalogue of rows, the shortcut and scheme tables |
| `crates/sshee-desktop` (root) | no | outside the window: system notifications whose click comes back (D-Bus, WinRT toasts, `UNUserNotificationCenter`), the tray's menu, the macOS menu bar item |
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
- **More than one window.** Shift+click on a server opens another `Shell` in the same process
  (`shell/windows.rs`): one backend shared by every window (`Shared`), tab ids in a range per
  window (`TabId::window`), and the backend's `Remote` routes through the `Hub`: an answer for
  a tab to that tab's window, news of the app (servers, vital signs, Tor) to every window,
  anything else to the window in front. A new `Remote` method picks one of the three.
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
  hold one; the terminals follow `Shell::look()`. It is Settings' `streamerMode`: the eye, its
  shortcut and the switch in Settings all go through it (`Shell::toggle_streamer`,
  `Shell::change_settings`), so it is one thing, and kept between runs.
- **Copying**: `PaneOptions::copy_on_select` and `program_clipboard` (a program's OSC 52), set
  for every pane by `Shell::set_copy_modes`, from Settings → Terminal → Copying
  (`SettingsStore::copy_on_select`, `program_clipboard`). A program that has the mouse gets the drag;
  Shift makes it the terminal's own selection.

## Tokens, icons, components

- `sshee-ui/src/theme.rs`: every colour (both appearances, from the design's stylesheet), size,
  radius and text style. `Theme::of(cx.env().system_dark)`. No colour or size is written
  anywhere else. The design: `research_notes/v1-plan/fresh-apple.html` (local notes).
- `sshee-ui/src/icons.rs`: `Icon` and `icon(Icon, size, colour)`; a new icon is one arm of
  `Icon::drawing` with SVG path data (`svg.rs` reads it), on the 16-unit grid, 1.4 pt line.
- `sshee-ui/src/kit/`: one module per component (`button`, `tab`, `rail`, `chip`, `menu`,
  `popover`, `status`; for forms and lists `field`, `list`, `tile`, `push`; and Settings'
  `switch`, `segmented`, `slider`, `popup`, `keycap`, `form`; over the window `toast` and
  `dialog`). A component is a function from what it shows to an `El`; the caller
  chains `.key(..)` and handlers. Add states to the component, not at the call site.
- From Rune itself, used as is: tooltips (`El::help`), focus rings and Tab order, text fields
  (`rune::text_input`), native pop-up menus, virtual lists (`El::virtual_rows`). **rune-kit**
  (menus, popovers, dialogs, tables, trees, Markdown, in OpenCode's look) is not a dependency:
  it pulls rune-highlights (a WASM-to-Rust build) and its own theme. The first screen that
  needs a big piece of it (a table for Files, Markdown for previews) should add it as a git
  dependency at Rune's pinned rev and wrap it in `kit/`; small things are cheaper to draw here.

## Settings

- **Where they live.** `Shell::settings` is a `sshee_app::settings_store::SettingsStore`: every
  0.10 setting (the typed `model::Settings`, 0.10's own shape on disk) plus what 1.0 added
  (`Additions`). The window gets it from its `Backend` (`Backend::settings`): the program's is
  `settings.json` in the copy's own folder, a demo's and a test's live in memory. A first start
  with no file waits, private, for `Shell::seed_settings` (what `import` made of 0.10's).
- **Reading one.** `s.settings.get().terminal_font_size`, `s.settings.more().tabs_name_server`.
  For what the window itself draws by them there are helpers: `s.dark(system_dark)` and
  `s.sidebar_at_right()` (`screens/settings/apply.rs`), `s.streaming()`, `s.server_shown(key,
  name)` and `s.text_shown(text)` (`shell/streamer.rs`).
- **Changing one.** `s.change_settings(tx, |st| st.set(|v| v.cursor_blink = false))`. The store
  tells its listeners and writes the file on its own thread; `Shell::apply_settings` then hands
  the values on: to the panes (`TermPane::set_options`, `set_look`), to the shortcut table, and
  to the backend (`Backend::settings_changed`) for what only the program can do.
- **Adding a row to Settings.** One `Row` in `settings_store/catalog.rs`: its section, name,
  help, control (`Switch`, `Choice`, `Slider`, `Text`, `Toggles`, `Buttons`) with a getter and a
  setter, the 0.10 keys it owns, its search words. The page draws the list; a test there fails
  if a 0.10 setting has no row. A feature that isn't built yet gets `.later("…")`: its row is
  drawn disabled with that one line. When the feature lands, drop the `.later` and make
  `apply_settings` (or your screen) read the value.
- **Themes.** `settings_store::looks` holds 0.10's themes as fourteen colours each (SSHeesh
  Default and Daylight are the design's dark and light), yours over a base, sharing as text and
  the accent; `sshee-ui/src/looks.rs` turns them into a `&'static Theme` (`Shell::theme`), the
  design's own when nothing differs. The editor and the colour picker are
  `screens/settings/looks.rs`. Features built elsewhere read their settings through the
  accessors on `SettingsStore` (`notify_attention`, `server_load`, `update_mode`, `tips_on`…).
- **When in doubt, private.** If the settings from before can't be read with certainty,
  streamer mode and secret hiding start on and `SettingsStore::notice` says why; the shell
  draws it across the top of the page (`screens::settings::notice_bar`) until "OK".

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

## Files, as the example of a screen with a backend

- **Rules** (`crates/sshee-app/src/files/`, `paths.rs`, `dir_cache.rs`, `transfers.rs`): `Browser`
  is one tab's folder (rows, selection, history, what a key does), `Editor` the open file and
  its save (overwrite, load theirs, save as a copy), `menu` what a right-click offers, `ops`
  every question and note, `protocol` what the screen asks the backend (`Ask`) and hears
  (`Reply`). No Rune, no sshee-core: sshee-core's types are mirrored there.
- **Seam** (`sshee-ui/src/screens/files.rs`): one function per thing the screen does (`go`,
  `open`, `save`, `rename`, `delete`, `download`...). Each changes the state at once and sends
  an `Ask` through `Shell::files_ask`; `deliver` puts the `Reply` (from `Remote::files`) back.
- **Drawing** (`screens/files/view.rs`, `screens/transfers.rs`): reads the state, calls the seam.
  Keys come through `Shell::key` (nothing in the list takes focus); what is over the window
  (menu, question, bookmarks, transfer cards) is drawn from `shell/view.rs`'s root.
- **Backends**: `desktop/app/src/files.rs` runs every `Ask` on its own thread on sshee-core;
  `demo/files.rs` is a made-up disk in memory with a fixed clock.
- **Between runs**: bookmarks and recent folders are `bookmarks.json` in the copy's folder (`desktop/app/src/bookmarks.rs`, `Backend::bookmarks`), brought over from 0.10 once. Files dropped from another app land through `El::on_drop_files` on the list (`screens::files::dropped`), so the platform's drag and drop is the only part left to Rune. Rules for what fits a pane's width are `files::layout`, Markdown is `files::markdown`.
- **Signed in once, signed in everywhere** (`desktop/app/src/signin.rs`): macOS and Linux
  share the terminal's ssh connection; Windows' OpenSSH can't, so the password that got a
  terminal in (typed at ssh's own prompt while connecting, or the saved one) is kept in
  memory for the run and handed to every other ssh for that server through askpass (Files,
  the edit bridge, the status bar, new terminals). A session that fails to sign in forgets
  it. A Files tab that said "Sign in first" lists again once a terminal there is in
  (`screens::files::signed_in`).

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

## Updating itself

A build the canary workflow made knows its commit and run; one the release workflow made
knows it's a release (`SSHEESH_RELEASE=1`, `app/build.rs`; Settings → About shows
`Options::build`). At start and every four hours (through Tor while Tor is on) it reads its
channel's manifest (`sshee_app::canary::Channel::choose`, from Settings → Updates):

- test builds: the `canary` pre-release's `canary.json`, archives signed by the canary key
  (`update::CANARY_PUBLIC_KEY`, never the release key);
- releases: `update-v1.json` in the `channel-stable` release (`channel-rc` for a release
  candidate), signed with its archives by the release key 0.10 trusts
  (`update::channel::RELEASE_PUBLIC_KEY`, the public half in `src-tauri/tauri.conf.json`). The
  manifest's signature names the file and its version; versions compare by semver, never
  down unless this version is listed as pulled. Never `releases/latest`: that's 0.10's.

It downloads a newer archive, checks its size, SHA-256 and signature, and stages the program
next to itself; `screens::canary` says so across the top of the page ("Ask me" only says a
version is out, with a link). "Restart to update" (`Backend::restart_to_update`) swaps the
files and leaves a helper (the old program, `--canary-watch`) that starts the new one once the
window has closed and puts the old one back if it doesn't say it started within a minute. All
of it is `app/src/update.rs` and `update/`; the words are `sshee_app::canary`. "Never check"
turns the looking off; "Check for test builds" off moves a test build to releases. 0.10's
`latest.json` and `v*` releases are never touched.

## Known gaps in the shell (not screens)

- From the platform (Rune, `vendor/rune.patch`): files dropped from another app reach Files
  (onto the list, or onto a folder row) and terminals (`shell/file_drop.rs`: this computer's
  types the quoted paths) on macOS, Windows and X11, not on Wayland (winit 0.30 reports no
  drags there); rows dragged past the window's edge download (`Want::DownloadHere`: on
  Windows into the desktop or Explorer folder under the pointer, else Downloads); the mouse's
  back and forward buttons in Files; "Upload files…" picks several (`Tx::open_files`); SVG
  previews draw (Rune's `svg`, with local `<image href>` refused); the main window opens
  where it closed (`shell/place.rs`, `window.json`, kept on a screen that's there; not on
  Wayland, which hides positions). A lone Alt on Windows never opens the system menu: winit
  keeps `WM_SYSKEYUP` from `DefWindowProc` for a window without a menu bar, as Rune's are.
- Linux keeps the system's title bar above the window: Rune draws no frame there, and a
  borderless window can't be resized by its edges (Rune doesn't expose winit's
  `drag_resize_window`). macOS and Windows get the design's bar (`App::titlebar`); neither has
  been run yet, nor have their system fonts (`fonts::SYSTEM_FAMILIES`).
- The row of tabs (`shell/tabs.rs`) has its menu, "+" with its menu (a terminal or Files
  here or on another machine), drag to reorder and a list for tabs out of sight; a tab can't
  be dragged out into a window of its own. The title (`shell/title.rs`) names the machine and
  its address and opens its tools (Tunnels and Recipes join that menu when their screens
  land).
- Settings' "Where tabs go: Under each server" hangs each machine's tabs under it in the
  sidebar, as rows in the list and smaller icons in the rail (`screens/rail_tree/tabs.rs`, the
  first part of a server's tree, above its containers); the row along the top keeps only "+".
  Ctrl+Tab, the numbers, "Close other tabs" and "Close tabs below" then stay with the machine
  on screen, and closing its last tab goes to the main menu (`Workbench::tabs_under_servers`).
  With the sidebar hidden the tabs run along the top again.
- A terminal tab is named after its shell's folder when the shell says it (OSC 7, or a
  `user@host: ~/dir` title). In tmux on a server neither reaches the window (tmux keeps both):
  the agents' watch reads `pane_current_path` and should call `Shell::set_folder`, and
  `TermPane::set_foreground` with the program in front. The program in front is wired
  (`app/src/agents.rs` asks tmux, `screens::agent::foreground` names the tab "Terminal N
  (Claude Code)" and tells the pane); the folder isn't yet.
- A click on a file's path finds it on its machine (`terminal/session.rs`: tmux's folder for a
  relative one, then the server's `find_path`) and shows it in Files beside the terminal;
  `path:line` says its lines in a note, the editor doesn't mark them yet. A web link whose
  text hides where it goes opens on a second click, not from a dialog.
- 0.10's saved passwords are read from its keychain entry only after the owner's yes
  ("Use them here?", `Ask::UseOldPasswords`). A server's key is checked before signing in
  (`LiveLink::begin`, `hostkeys::check`) and asked about on the pane's card
  (`screens/prompts.rs`), so ssh never asks it, in the terminal or of askpass.
- Tor's round trip is the one sshee-core timed when it picked the route used last
  (`sshee_core::tor::latency_ms`); nothing is sent to measure it. Until a server has been
  reached through Tor, the status bar says "Tor" alone.
- A pane out of sight answers its program at once but takes a frame only every half second
  (`TermPane::set_shown`, `HIDDEN_FRAME`): enough for an agent's question and "went quiet".

## Splits, the sidebar and compact mode

- **Splits** are 0.10's tree (`layout.ts`): `sshee_app::workbench::layout` holds groups of tabs,
  each a tree of rows and columns with shares, kept in `Workbench` (so closing a tab gives its
  room to the panes around it and brings up the pane nearest it) and saved in `tabs.json` with
  the tabs (`SavedTabs::splits`, by place in the row). Every pane stays a tab of its own;
  `Workbench::on_screen` is what the page draws. Split right and down (⌘D, ⇧⌘D; Ctrl+Shift+D,
  Ctrl+Shift+E), "Fill the window" (⇧⌘↩, Ctrl+Shift+Enter: never an Enter in the terminal) and
  "Show all N panes", side by side and one at a time (the group is kept), Settings' "Tabs on
  screen" and "Arrange open panes", a tab or a pane's header dropped on a side or the middle of a
  pane (`layout::zone`, `Layout::place`), and two servers on one page with each pane naming its
  own. `shell/split.rs` draws it: `side-N` (header `side-head-N`, page `pane-N`), `split-…` and
  `divider-…` by their path in the tree; "Space between panes" is the dividers' room.
- **The sidebar** (`shell/rail.rs`) is the rail of icons or 0.10's list of names; its edge drags
  (`layout::sidebar`: under 120 points it folds into icons) and Settings keeps `sidebarMode` and
  `sidebarWidth`. Hidden or shown by its button, ⌘B / Ctrl+Shift+B and the palette. A server
  shows its state (`Workbench::health`: connecting, lost, couldn't connect), its label (Prod,
  Staging, Dev), its load as Settings' `railStatus` picks (`shell/rail_load.rs`), and in the
  list a "+" for a new terminal and its containers (`screens/rail_tree.rs`, the Containers
  tab's `Board` under an id no tab has). Pixel icons: `pixel_icons.rs`.
- **Compact mode** (`shell/compact.rs`, ⇧⌘M / Ctrl+Shift+M): the sidebar, the top row or both
  stand aside, and slide in over the page while the pointer is at that edge.
- Files' own shortcut is Ctrl+Shift+O off a Mac (⇧⌘E on one): 0.10's Split down has Ctrl+Shift+E.

## Screens that are built

| Screen | In `sshee-ui/src/` | Rules in `crates/sshee-app/src/` |
|---|---|---|
| Main menu: search field, server tiles, agents at work, closed tabs | `screens/home.rs` | `home.rs` |
| Command palette (`srv:` `cmd:` `file:` `set:`; a command for a server is never preselected) | `screens/palette.rs` | `palette.rs` |
| Add a server: by hand (keys found in ~/.ssh, password managers, the eye, "Check before adding", icon, colour and label right away; the body scrolls in a low window), from the ssh config, XPipe and 0.10 | `screens/add_server.rs` | `servers.rs` (`Form`, `Ask`, `Answer`, and `Book`, the list in memory a demo and the tests answer from) |
| A server's menu (New terminal, Files, terminal and files side by side, Stats, Containers, Tunnels, Copy address, rename, the look picker with Automatic, any colour and a picture, label, Forget saved password, remove; a change that fails says so) and its card under the pointer; every server's face (`State::faces`: its colour and picture on the sidebar icon) and the icon guessed on the first connect | `screens/server_menu.rs`, `screens/server_card.rs` | `labels.rs`, `roles.rs`, `home.rs` (`Card`) |
| Security check: findings worst first in plain words, each fix shown in full, run only after a yes and on Production after a second one; a fix that needs you opens in a terminal. From the sidebar's shield (a list of servers from the main menu) and the palette | `screens/checkup.rs` | `recipes.rs`, `recipes/checkup.rs` |
| Free up disk space: each disk's used of total with a meter, the safe cleanups (strengths to pick, each command in full, run after a yes and on Production a second one, what it freed said after), the biggest folders one level at a time, Docker, the journal. From the sidebar's drive, the top row, the palette, a server's menu and Stats | `screens/disk.rs` | `disk.rs` (`Report`, `Ask`, `Reply`, `freed`) |
| Recipes: eight step-by-step guides (0.10's, word for word), the server's system guessed from its kernel, fields, the key from `~/.ssh`, Paste into terminal without Return, the bar it steps aside into | `screens/recipes.rs` | `recipes.rs`, `recipes/book.rs` |
| Snippets: the list, the editor (one server or every server), the blanks asked before it goes into the terminal; Production asks before Return | `screens/snippets.rs` | `snippets.rs` |
| Welcome (first start: bring 0.10's servers over, pick from `~/.ssh/config`, add one), the tour (the simple one, Р7), hints, What's new (CHANGELOG.md built in) | `screens/welcome.rs`, `screens/tour.rs`, `screens/whats_new.rs` | `tour.rs`, `changelog.rs` |
| Containers (a tab kind): grouped by Compose project, state, ports, image, uptime, health; Start, Stop, Restart; logs (followed) and a shell, each in a new terminal tab (`Tab::container`, kept between runs); trouble said with the command that fixes it | `screens/containers.rs` | `containers.rs` (`Board`, `Ask`, `Reply`) |
| Stats (a tab kind): CPU, load, memory, network and each disk with graphs over five minutes or meters, the machine's name and kernel, the busiest programs; the load on a server's card in the sidebar | `screens/stats.rs`, `kit/graph.rs` | `stats.rs`, `workbench/vitals.rs` (`History`) |
| Tor: the popover over the sidebar's onion and the status bar's "Tor 84 ms" (the switch, a start's progress and what holds it up, the round trip, the log's last lines, new routes, start fresh) | `screens/tor.rs` | `tor.rs` |
| Tunnels, from a server's menu and the title's: saved per server with their switches and states; a server's port here, a SOCKS proxy, and a port here on the server (1.0's own). Not built: 0.10's list of the server's web apps to open with one click | `screens/tunnels.rs` | `tunnels.rs` |

Tor: `desktop/app/src/live.rs` hears sshee-core's watcher (`Remote::tor_status`), looks at
the round trip every 2 s (`Remote::tor_latency`), and after a switch moves what's connected
to the new way as 0.10 does (`SessionManager::reroute`, Files' shells, tunnels, shared
connections). `ssheesh-preview --tor-check` starts this copy's Tor without a window and says
each step: the first thing to run where Tor won't start. Tunnels: `desktop/app/src/tunnels.rs`
(sshee-core's `TunnelManager`, the list in `~/.sshee-preview/tunnels.json`, 0.10's shape).

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
| Files, what is left (everything of 0.10's Files is built but these): a video player (Rune has no video element), the editor over 128 KB (Rune's text field shapes the whole text on every key), a card flying with the pointer while rows are dragged, drag onto a Files tab in the row of tabs | `screens/files.rs`, `screens/files/` | `files.rs`, `files/`, `paths.rs`, `dir_cache.rs` |
| Transfers, what is left: notes that go away by themselves (`Transfers.tsx`, toasts of `Overlays.tsx`) | `screens/transfers.rs` | `transfers.rs` |
| Settings: built, themes and their editor included; a row marked `.later` in the catalogue would wait for another feature (none is today) | `screens/settings.rs`, `screens/settings/` | `settings_store.rs`, `settings_store/` |
| A tab dragged out to its own window (`windows.ts`) | `shell/tabs.rs` | `workbench/layout.rs` |
| An agent's question and answers; agent names on tabs (`agents.ts`, the design's popover). Built: names and "Needs you" on tabs, the card under a waiting agent's tab, the list of agents (Ctrl+Shift+A); the question is read by `sshee-term::question`. Not built: the buttons that start an agent | `screens/agent.rs`, `screens/agent/` | `agents.rs`, `agents/answer.rs` |
| Host-key question, reconnect and sleep notices, the agents' buttons: built (`terminal/session.rs` holds their state). Not built: a password question of its own | `screens/prompts.rs` | `prompts.rs` |
| In the terminal, still to do: highlights (`Look::highlights`, drawn as `Role::Paint`), the recap's full list of lines, 0.10's question about which folder files dropped on a server's terminal go to (`UploadDialog`: today they go to the folder the shell said it's in, else Files opens there; `shell/file_drop.rs`) (find, links, away, the scroll bar, the menu and streamer mode are built) | `terminal/` | none (sshee-term has them) |
| Security check and recipes, what is left: `{{container}}` in a snippet as a list of the server's containers (it's a text field until Docker's screen can list them) | `screens/checkup.rs`, `screens/recipes.rs` | `recipes.rs` |
| Docker, what 0.11 adds (README's roadmap): per-container stats, updates you can roll back, systemd services; sshee-core has none of it yet | `screens/containers.rs` | `containers.rs` |
| Welcome, what is left: 0.10's "Make it yours" page (look, layout, privacy with a live preview), the "what changed" screen for 0.10 users, the tour's animated scenes (`TourScenes.tsx`) | `screens/welcome.rs`, `screens/tour.rs` | `tour.rs` |
| Second window, the mini window (`windows.ts`, `MiniApp.tsx`) | `shell/windows.rs` | `windows.rs` |

Bringing settings and tabs over from 0.10 at first start (`sshee-app::import`) isn't wired.
Servers, with their labels and looks, come over when you pick them in "Add a server"
(`desktop/app/src/servers.rs`). Snippets are this copy's `snippets.json` (`desktop/app/src/recipes.rs`),
brought over from 0.10 at the first start with the settings (`Shell::seed_snippets`); the palette offers
`screens::snippets::offered`.

## The security check, recipes, snippets and the first start

- **Backend.** `Backend::recipes(Ask)` (`sshee_app::recipes::Ask`: `Check`, `Fix`, `Facts`,
  `ReadKey`) answers through `Remote::recipes`, delivered by `screens::checkup::deliver` (which
  hands recipe facts and keys to `screens::recipes::deliver`). The live one is
  `desktop/app/src/recipes.rs` on `sshee_core::checkup`, `fixes`, `metrics` (the kernel) and
  `keys`, each on a thread with the server's saved password read there; the demo's is
  `app/src/demo/recipes.rs`. `Backend::snippets` / `snippets_changed` keep the list;
  `Backend::welcome` says whether this start may show the welcome (the program yes; a demo with
  `--demo-welcome`, a test when it asks); `Backend::version` lets "What's new" open by itself
  after an update (`--demo-whats-new`).
- **Nothing runs on its own.** A fix needs Run, then a yes (`Gate::Confirm`), and on a server
  labelled Production a second yes (`Gate::Production`), which Return doesn't press. A recipe's
  step is pasted without Return. A snippet that runs with Return on Production asks first.
- **Dialogs** are drawn by `screens::checkup::dialog` (scrim, modal, Esc), with `header`,
  `command_block` and `note` shared by the five; each is in `States::any_open` and
  `close_all`. The recipe stepped aside and a hint are drawn over the page and take no keys.
- **Where it's opened:** the sidebar's shield (`Shell::open_checkup_here`), the palette's rows
  (`Action::Checkup`, `Recipes`, `Snippets`, `NewSnippet`, `WhatsNew`, `Tour`), a finding's
  "Open the recipe". Settings → About's "Take the tour" and "What's new" can call
  `Shell::open_tour(0, tx)` and `Shell::open_whats_new(tx)`.

## Containers and Stats, and how their looks are timed

- **Containers**: `Backend::containers(Ask)` (`app/src/containers.rs` on sshee-core's
  `containers`, each request on its own thread), answered through `Remote::containers`, which
  hands `screens::containers::deliver` a `Tx` to time the next look (`Tx::wake_at`, 10 s,
  longer while looks fail). `screens::containers::tick` (from `Shell::poll`) asks when a tab on
  screen is due; a tab out of sight doesn't look. A terminal into a container is a Terminal tab
  with `Tab::container`: the live backend runs sshee-core's `containers::argv` (only for an id
  that reads as one), the demo plays a prompt or a log.
- **Stats**: the vitals thread (`app/src/vitals.rs`) looks at the machine on screen every 2 s
  and at the others with a connected terminal (`Backend::agents_on`) every 10 s; each look gives
  `Remote::set_vitals` (the status bar, `Workbench::history`) and `Remote::set_facts` (name,
  kernel, load, every disk: `States::machines`). The busiest programs (`sshee_core::metrics::processes`,
  one `ps`) are asked through `Backend::stats` every 5 s while a Stats tab is on screen. A
  server's card shows its load only while it is being looked at, never old numbers.

## Toasts and confirmations

`shell/notes.rs` holds them; the cards are `kit/toast.rs` and `kit/dialog.rs`.

- **A toast**: `s.toast(Toast::success("Downloaded site.zip").body(..).action("Show in folder",
  |s| ..))`. It stacks in the bottom right corner under the transfer cards
  (`screens::transfers` draws both), leaves by itself (3.8 s, 9 s with a button, 7 s for an
  error; longer while the pointer rests on it), at most four at a time. Files' notes
  (`sshee_app::files::Notice`) are toasts now (`screens::files::note`).
- **A confirmation**: `s.confirm(Confirm::new(title, body, "Remove").destructive(undoable),
  |s, tx| ..)`; the closure runs on yes only. One at a time, over a dimmed window; it takes
  the keyboard (`States::any_open`); Return presses the default button, Esc cancels. What
  can't be undone makes Cancel the default. A screen with its own question state draws the
  same card with `kit::dialog::dialog` (Files' `question`).
- Both go through `Shell::text_shown`: streamer mode dots names in them.

## The desktop outside the window

Notifications and the tray are `crates/sshee-desktop` (no Rune; its doc comment says what each
system does); the window's side is `sshee-ui/src/shell/desktop.rs`; the program joins them in
`desktop/app/src/desktop.rs` (the live backend only; nothing starts in a headless run).

- **A notification**: `screens::agent` hands a safe note to `Shell::notify_system`, which
  honours Settings' "Tell me when a terminal needs me" and calls `Backend::notify`. Its click
  comes back as `Remote::open_tab(tab, token)`: Rune's `AppHandle` shows and raises the window
  (with the Wayland token when there is one), and `Shell::open_from_outside` closes what is
  open over the page and puts the tab on screen with the keyboard. On Windows the click is a
  link (`ssheesh-preview://notification/<tab>`): Windows starts the program, Rune's
  `single_instance` passes it to the running one (`App::on_open_url`, `Shell::open_link`).
- **The tray**: `Backend::waiting_changed` gets the tabs that need you (names as shown, so
  dotted while streaming) after every change; the tray lists them, and a tab you look at takes
  its notification away. Its menu: Show or Hide, the tabs that need you, Quit. Rune's tray on
  Windows and Linux (`shell::desktop::SystemTray`), sshee-desktop's menu bar item on macOS.
- **What isn't verified**: only Linux has run it (D-Bus notifications, the tray needs a host:
  KDE, a bar, GNOME's extension). Windows and macOS build (`cargo check --target`) but haven't
  run. macOS shows notifications only from an `.app` bundle; the preview isn't one yet, so
  there the Dock icon bounces instead.

## Open in your own editor (for the Files screen)

Built, core and screen. The screen's rules are `sshee_app::files::edit` (which editor, each
status's words, the questions, `EditAsk`/`EditReply`); the seam and the drawing are
`sshee-ui/src/screens/files/edit.rs` (`Backend::edit`, `Remote::edit`); the live backend is
`desktop/app/src/edit.rs` and the demo's `app/src/demo/edit.rs` (no editor starts; the status
flow plays, `--demo-leftovers` offers two leftovers). The chosen editor is 1.0's own setting
`externalEditor` (Settings → Files → "Open files in"). Not built yet: a submenu for "Open
in…" (it replaces the menu in place), and Upload or Discard per leftover (the prompt acts on
all). `sshee_core::edit_bridge` copies a server's
file into a private folder, starts an editor on it and uploads every save with a guarded save;
`sshee_app::editors` finds the editors on this computer. Neither knows about Rune. The rules
(sizes, how long copies live, the start-up sweep, the 2 GB cap, safe deletion) are in the doc
comment at the top of `edit_bridge.rs`; what the window does with them:

- **One bridge per app**, held by the backend: `EditBridge::for_app(sink)` (the edit folder is
  `<the copy's folder>/edit`). Its `sink` gets `(session id, EditEvent)` on the bridge's
  threads: pass them on through `Remote`. Every call that talks to a server blocks, so the
  backend runs it on a thread, as with the rest of Files. Each server's `Remote` for the bridge
  is `edit_bridge::Server { target, env }`, the same `Target` and `SshEnv` Files uses, so Tor
  and saved passwords apply as they do there.
- **The menu item.** In a file's menu: "Open in <name>", with `name` from
  `editors::chosen(Os::current(), &Places::here(), setting, &editors::installed())` (setting
  `externalEditor`), and under it the other editors found and "Choose an app…" (a file picker;
  `editors::choose_app` checks the pick and gives the id `custom:<path>` to store). Build the
  bridge's `EditorCommand { program, args }` from the `Editor`. Call `bridge.open(OpenRequest
  { host, path, remote, editor, confirmed: false })`.
- **The 128 KB rule.** Below `edit_bridge::BUILT_IN_MAX` (128 KB) Files' own editor stays the
  default action; at or above it, the preview says "This file is too big to edit here" with the
  button "Open in <name>". The menu item is there for any size.
- **Big files.** `Opened::NeedsConfirm { size }` (over 50 MB): ask "This file is 230 MB.
  Download a copy to edit?" and on yes call `open` again with `confirmed: true`. Over 1 GB, and
  when the copy would leave less than 1 GB free, `open` fails with a message to show as it is.
- **Before opening, once:** while Tor is on, warn that the editor, its extensions and AI
  helpers reach the internet from your real address (offer Files' own editor instead); in
  streamer mode, warn that the editor shows the file as it is, secrets included.
- **The status line** under the file's row (and in the preview's header) is the session's last
  event: `Opened` "Open in VS Code", `Uploading` "Saving to the server…", `Uploaded { at }`
  "Saved to the server at 12:41", `Offline` "Not on the server yet: waiting for the connection"
  (with a Retry button: `retry_now`), `Failed { message }` the message and Retry, `Evicted`
  "Copy cleared to save space; Open downloads it again", `Closed`. A "Stop editing" button on
  it calls `close(id)`; when `kept` comes back, say where the copy is ("Show in folder").
  `bridge.sessions()` lists the open ones (`SessionInfo`) for a Files view opened later.
- **The conflict** (`EditEvent::Conflict`): a dialog "This file changed on the server since you
  opened it" with **Keep mine** (`keep_mine`), **Take theirs** (`take_theirs`; the editor
  reloads it), **Save mine as a copy** (`save_mine_as_copy`, which returns the new path; then
  `take_theirs`), and **Compare** (`compare`: both texts and `lines`, a line diff marked
  `Same`/`Mine`/`Theirs`; `binary` when there's nothing to diff). With `now: None` the file
  was deleted on the server, and Keep mine puts it back.
- **When sessions end:** "Stop editing" (`close`); when a server's last tab closes
  (`close_host(host)`); on quit (`close_all`, before the window goes). Reconnecting a server
  calls `retry_host(host)`. A session also closes by itself after 24 hours without a change and
  with nothing to upload.
- **At start:** `bridge.sweep()` deletes uploaded leftovers and returns the others. When
  there are any, show once "2 files have changes that were never uploaded" with **Upload**
  (`resume_leftover(id, remote, None)`; it can raise a conflict like any session), **Discard**
  (`discard_leftover`) and **Show in folder** (`local`), and call `mark_offered(ids)` when it's
  shown: one that's offered and then sits for 30 days is deleted at a later start.
