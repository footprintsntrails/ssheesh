# 0.10's web view stores, for `legacy`

Each folder is one computer's worth of 0.10 leftovers, laid out as on that system below the base
folders that `legacy::Places` takes:

| Folder | Base folders | Read with |
|---|---|---|
| `windows-*` | `Local` = `%LOCALAPPDATA%`, `Roaming` = `%APPDATA%` | `Places::windows(Local, Roaming)` |
| `linux-*` | `data` = `$XDG_DATA_HOME`, `config` = `$XDG_CONFIG_HOME` | `Places::linux(data, config)` |
| `macos-*` | the home folder | `Places::macos(home)` |

Everything in them is made up: servers `web-7731` (203.0.113.17), `db-main` (198.51.100.23) and
`staging-api` (203.0.113.40), a snippet holding the fake token `tok_made_up_4f7a1c9e2b`.

`.gitattributes` here keeps every file byte for byte, whatever a checkout's `core.autocrlf` says.
LevelDB's `CURRENT` is plain text: checked out with CRLF on Windows, it names a manifest that
isn't there, and every LevelDB test fails.

## What each one is, and how real

| Fixture | Written by | Real or built |
|---|---|---|
| `linux-webkitgtk` | the real SSHeesh 0.10.14 under WebKitGTK 2.52.6 (webkit2gtk-4.1), closed normally | **real**: the app wrote every byte |
| `linux-webkitgtk-running` | the same app, its store copied while it was open (`-wal` and `-shm` beside the database) | **real** |
| `windows-sqlite` | Chrome for Testing 158.0.8127.18, Chromium's SQLite localStorage (Chrome's default for a new profile), closed normally | **real Chromium**, not WebView2 |
| `windows-sqlite-running` | the same, copied while Chrome was open (`LocalStorage` + `LocalStorage-wal`) | **real Chromium** |
| `windows-leveldb` | Chrome for Testing 158 with SQLite localStorage turned off, so LevelDB; copied while Chrome was open | **real Chromium** |
| `macos-webkit` | built: the Linux fixture's real WebKit database and `origin` file, placed in newer WebKit's macOS layout (`WebsiteData/Default/<hash>/<hash>/LocalStorage/localstorage.sqlite3`) | layout from WebKit's source, **not seen on a Mac** |
| `macos-webkit-older` | built: the same database in older WebKit's layout (`WebsiteData/LocalStorage/tauri_localhost_0.localstorage`) | layout from WebKit's source, **not seen on a Mac** |
| `.window-state.json` under `Roaming` and `Library/Application Support` | written by hand in tauri-plugin-window-state 2.5.0's shape | built |

WebView2 is Chromium, and its localStorage is Chromium's own code (`components/services/storage/
dom_storage/`), so Chrome's files are the same formats. What they can't show is which of the two
formats a given WebView2 runtime uses, and that WebView2 names its profile folder `Default`.

What these files confirmed, beyond reading the sources:

- origins: `tauri://localhost` (WebKitGTK, 0.10.14's real page), `http://tauri.localhost` (in
  Chromium's LevelDB keys), `http://tauri.localhost/` with a slash (Chromium's SQLite `maps`);
- Chromium's LevelDB keys `_` + origin + `00` + `01` + key, and `META:` / `METAACCESS:` records;
- Blink's first byte: `01` (Latin-1) for ASCII and for `é`, `00` (UTF-16) once there is Cyrillic;
- Chromium's SQLite compresses values of 350 bytes or more with zstd (settings and workspace
  here), and leaves one uncompressed when zstd doesn't save a fifth (the Cyrillic snippets);
- WebKit keeps values as UTF-16 BLOBs, runs in WAL mode, and while the app is open the database
  file holds only the empty table: the records are in `-wal`;
- WebKit's `origin` file: for each of top origin and origin, scheme and host as 32-bit length,
  an 8-bit flag and the characters, then a port presence byte;
- 0.10.14 writes `.window-state.json` only when the window closes.

Still wanted from real machines (see the plan, section 6.5): a whole
`%LOCALAPPDATA%\io.github.footprintsntrails.ssheesh\EBWebView\` from Windows 10 and 11 with the
WebView2 runtime version, a whole `~/Library/WebKit/io.github.footprintsntrails.ssheesh/` with
the macOS version, and a store from an older 0.x if one is still around.

## How they were made

Scripts and pages are in `make/`.

### Linux, the real app

1. The 0.10.14 release tree (`git archive 46a6aeb`) in a scratch folder; `npx vite build` there;
   then `cargo build -p ssheesh --features tauri/custom-protocol`. The feature gives the page the
   origin releases have, `tauri://localhost`; a plain debug build loads `http://localhost:1420`
   from Vite instead.
2. A made-up profile: `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` and
   `XDG_RUNTIME_DIR` in a scratch folder, `~/.ssh/config` with `web-7731` and `db-main`, and a
   `hosts.json` with `staging-api`.
3. Xvfb on `:111` (1600x1000). The app ran with every `*_PROXY` variable set to
   `http://127.0.0.1:9`, so it had no network and the updater couldn't reach GitHub;
   `no_proxy=127.0.0.1,localhost`; `WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9339`;
   `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
4. The welcome dialog through `xdotool`: "Add 2 servers", the Midnight theme, the blue accent,
   Dracula terminal colours, "Start using SSHeesh".
5. `node make/wk-eval.mjs make/webkit-seed.js`: runs the script in the app's page through
   WebKit's remote inspector. It sets update mode to manual, streamer mode on, a welcome note with
   Cyrillic, `é` and `☕`, labels, snippets, bookmarks, tunnels, typing counts and a workspace
   with two terminals on servers (session keys `q8x2m4k7za`, `t5n1b9w3ce`) and a files tab, then
   reloads the page, so the app read them back through its own stores. It then tried the
   made-up servers and stopped at "ControlPath too long", without touching the network.
6. In the app: "This computer" on the rail, then Ctrl+Shift+T: two local terminals, with the
   app's own session keys `s9vfdo66z2` and `9ruv7f27vr`.
7. `linux-webkitgtk-running`: the `localstorage` folder copied with `cp` while the app was open.
8. The app closed through `make/wm-delete.py` (sends `WM_DELETE_WINDOW`, as a window manager's
   close button does; there was no window manager, and Ctrl+Q does nothing on Linux). WebKit
   folded the WAL into the database on the way out: `linux-webkitgtk`. With no window manager
   to place the window, the window-state plugin wrote `{}`.

Kept from the profile: the store, the `storage/<hash>/<hash>/origin` file, `hosts.json`,
`.window-state.json`. Left out: WebKit's salts, `hsts-storage.sqlite` and caches.

### Chrome

`make/chrome-*.html` were served with `python3 -m http.server 8411 --bind 127.0.0.1`. Each run
was one headless Chrome loading one page and quitting normally:

```
chrome --headless --no-sandbox --disable-gpu --no-first-run --password-store=basic \
  --use-mock-keychain --user-data-dir=<profile> \
  --host-resolver-rules="MAP tauri.localhost 127.0.0.1:8411, MAP other.localhost 127.0.0.1:8411" \
  --virtual-time-budget=4000 --dump-dom <url>
```

The resolver rule gives the page the origin `http://tauri.localhost`, as WebView2 gives 0.10's.

- SQLite: `chrome-seed.html`, then `chrome-update.html` (a second terminal, a newer workspace,
  and `sshee-typing-counts` removed): `Default/LocalStorage` is `windows-sqlite`. Then
  `chrome-live.html` with `--remote-debugging-port=0` instead of `--dump-dom`, so Chrome stayed
  open, and the files copied after 7 seconds: `windows-sqlite-running`. There is no `-shm`:
  Chromium opens its databases in exclusive locking mode.
- LevelDB: the same with `--disable-field-trial-config
  --disable-features=DomStorageSqlite,DomStorageSqliteNewDatabases,DomStorageSqliteMigration`.
  `chrome-seed.html`; `chrome-junk.html` (another origin, `http://other.localhost`, writes about
  4 MB, so at the next start LevelDB turns its log into the Snappy-compressed table
  `000003.ldb`); `chrome-update.html` (a new log, `000004.log`, with the newer workspace and
  the deletion); `chrome-live.html`, copied while open: `windows-leveldb`. Chrome's `LOG` and
  `LOG.old` are left out, as they name folders on the machine that made them.

The profile files went below `Local/io.github.footprintsntrails.ssheesh/EBWebView/Default/`,
where WebView2 keeps the profile Tauri gives it.

LevelDB logs end in `.log`, which the repository's top `.gitignore` drops; the `.gitignore`
here brings them back.
