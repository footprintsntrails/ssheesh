//! `--demo`'s files: a made-up tree per server, kept in memory, that answers everything a
//! Files tab asks (`sshee_app::files::Ask`) the way a server would. Listing, opening, saving,
//! new folders, renaming, deleting, moving and unpacking change the tree; nothing on this
//! computer or any other is read or written, and the clock is fixed, so every run draws the
//! same.
//!
//! Two things are staged so their screens can be shown: saving `~/shop/CHANGELOG.md` always
//! answers that it changed on the server (a colleague's line appears in it), and downloading
//! `~/backups/shop-db.sql.gz` stays at 38% until it's stopped.

use std::collections::{BTreeMap, HashMap};

use sshee_app::files::protocol::{Answer, Ask, Reply, SaveFailure, Want};
use sshee_app::files::{Clock, Entry, EntryKind, Failure, FailureKind, Listing, Stamp, TextFile, MAX_TEXT};
use sshee_app::paths;
use sshee_app::transfers::{Progress, TransferId};

/// The demo's "now": 2024-10-12 14:02 UTC, and this computer is on UTC.
pub const NOW: Clock = Clock { now: 1_728_741_720, utc_offset: 0 };

const MINUTE: i64 = 60;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// The file whose saves always find it changed on the server.
pub const CONTESTED: &str = "/home/deploy/shop/CHANGELOG.md";
/// The download that never finishes by itself.
pub const ENDLESS: &str = "/home/deploy/backups/shop-db.sql.gz";
/// What a stamp from a "changed on the server" answer carries, so saving over it knowingly
/// goes through.
const SEEN_THEIRS: &str = "demo: the colleague's version";
/// Where the demo says downloads landed. Made up: nothing is written.
const DOWNLOADS: &str = "/home/dev/Downloads";

#[derive(Clone)]
struct Node {
    kind: EntryKind,
    is_link: bool,
    bytes: Vec<u8>,
    /// A size to show that isn't the content's (a big file with nothing behind it).
    size: Option<u64>,
    modified: i64,
    mode: &'static str,
}

/// One server's made-up disk: every folder and file by its full path.
pub struct Disk {
    nodes: BTreeMap<String, Node>,
    home: String,
}

fn failure(kind: FailureKind, message: &str) -> Failure {
    Failure::new(kind, message)
}

fn not_found() -> Failure {
    failure(FailureKind::NotFound, "No such file or directory")
}

/// A small PNG made here: a rounded square in the design's accent on white. Stored without
/// compression, so it needs no library.
pub fn logo_png() -> Vec<u8> {
    const SIZE: usize = 48;
    let mut raw = Vec::with_capacity(SIZE * (1 + SIZE * 3));
    for y in 0..SIZE {
        raw.push(0); // no filter
        for x in 0..SIZE {
            let inside = (8..40).contains(&x) && (8..40).contains(&y) && !(!(12..36).contains(&x) && !(12..36).contains(&y));
            raw.extend_from_slice(if inside { &[0x0a, 0x84, 0xff] } else { &[0xff, 0xff, 0xff] });
        }
    }
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (!(crc & 1)).wrapping_add(1));
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    // zlib: header, one stored block (length and its complement), Adler-32.
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    zlib.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    zlib.extend_from_slice(&raw);
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut header = Vec::new();
    header.extend_from_slice(&(SIZE as u32).to_be_bytes());
    header.extend_from_slice(&(SIZE as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bits, RGB
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    png
}

const README: &str = "# shop\n\nThe shop's web app. Releases live in `shop/releases`, one folder per day;\n`shop/releases/current` points at the one being served.\n\n## Deploying\n\n    ./deploy.sh 2024-10-12\n\nLogs are in `logs/`. Ask before touching `nginx.conf`.\n";
const ENV: &str = "# Made-up values for the demo.\nAPP_ENV=production\nAPP_PORT=8080\nDB_HOST=203.0.113.31\nDB_NAME=shop\nDB_PASSWORD=not-a-real-password\nPAYMENTS_KEY=made-up-key-1234\n";
const NGINX: &str = "server {\n    listen 80;\n    server_name shop.example;\n    root /home/deploy/shop/releases/current/public;\n\n    location /api/ {\n        proxy_pass http://127.0.0.1:8080;\n    }\n}\n";
const DEPLOY: &str = "#!/bin/sh\n# Puts a release live: ./deploy.sh 2024-10-12\nset -eu\nrelease=\"$1\"\ncd \"$HOME/shop/releases\"\ntest -d \"$release\" || { echo \"no such release: $release\" >&2; exit 1; }\nln -sfn \"$release\" current\nsudo systemctl reload shop\necho \"now serving $release\"\n";
const CHANGELOG: &str = "# Changelog\n\n## 2024-10-12\n- Checkout: free shipping banner\n- Fixed the cart badge on small screens\n\n## 2024-10-05\n- New product page\n";
const WORKER: &str = "# Made-up settings for the demo's worker.\nqueue = \"orders\"\nthreads = 4\nretry_after_s = 30\n";
const THEIR_LINE: &str = "- Hotfix: prices round to two places (added on the server)\n";
const ACCESS_LOG: &str = "203.0.113.50 - - [12/Oct/2024:13:58:01 +0000] \"GET / HTTP/1.1\" 200 5120\n203.0.113.51 - - [12/Oct/2024:13:58:07 +0000] \"GET /cart HTTP/1.1\" 200 2048\n203.0.113.50 - - [12/Oct/2024:13:59:40 +0000] \"POST /api/checkout HTTP/1.1\" 201 312\n";

impl Disk {
    fn empty(home: &str) -> Disk {
        let mut disk = Disk { nodes: BTreeMap::new(), home: home.to_owned() };
        disk.dir(home, NOW.now - 2 * HOUR);
        disk
    }

    fn put(&mut self, path: &str, node: Node) {
        self.nodes.insert(path.to_owned(), node);
    }

    /// A folder, and every folder above it that isn't there yet.
    fn dir(&mut self, path: &str, modified: i64) {
        let mut at = path.to_owned();
        loop {
            self.nodes.entry(at.clone()).or_insert(Node { kind: EntryKind::Dir, is_link: false, bytes: Vec::new(), size: None, modified, mode: "drwxr-xr-x" });
            if at == "/" {
                break;
            }
            at = paths::parent(&at);
        }
    }

    fn file(&mut self, path: &str, bytes: impl Into<Vec<u8>>, modified: i64, mode: &'static str) {
        self.dir(&paths::parent(path), modified);
        self.put(path, Node { kind: EntryKind::File, is_link: false, bytes: bytes.into(), size: None, modified, mode });
    }

    /// The shop server: `/home/deploy`.
    pub fn web() -> Disk {
        let home = "/home/deploy";
        let mut d = Disk::empty(home);
        let at = |rest: &str| format!("{home}/{rest}");
        for (day, age) in [("2024-09-28", 14 * DAY), ("2024-10-05", 7 * DAY), ("2024-10-12", 3 * HOUR)] {
            d.dir(&at(&format!("shop/releases/{day}/public")), NOW.now - age);
            d.file(&at(&format!("shop/releases/{day}/VERSION")), format!("{day}\n"), NOW.now - age, "-rw-r--r--");
            d.file(&at(&format!("shop/releases/{day}/public/index.html")), "<!doctype html>\n<title>Shop</title>\n<h1>Shop</h1>\n", NOW.now - age, "-rw-r--r--");
        }
        // A link to the folder being served, and one whose target is gone.
        d.put(&at("shop/releases/current"), Node { kind: EntryKind::Dir, is_link: true, bytes: Vec::new(), size: Some(10), modified: NOW.now - 3 * HOUR, mode: "lrwxrwxrwx" });
        d.put(&at("shop/releases/previous"), Node { kind: EntryKind::BrokenLink, is_link: true, bytes: Vec::new(), size: Some(10), modified: NOW.now - 40 * DAY, mode: "lrwxrwxrwx" });
        d.file(CONTESTED, CHANGELOG, NOW.now - 3 * HOUR, "-rw-r--r--");
        d.file(&at("shop/package.json"), "{\n  \"name\": \"shop\",\n  \"version\": \"3.8.1\",\n  \"private\": true\n}\n", NOW.now - 7 * DAY, "-rw-r--r--");
        d.file(&at("logs/access.log"), ACCESS_LOG, NOW.now - 20, "-rw-r-----");
        d.file(&at("logs/error.log"), "", NOW.now - DAY - 5 * HOUR + 13 * MINUTE, "-rw-r-----");
        d.file(&at("logs/deploy.log"), "2024-10-12 11:02 now serving 2024-10-12\n", NOW.now - 3 * HOUR, "-rw-r--r--");
        d.file(ENDLESS, Vec::new(), NOW.now - 9 * HOUR, "-rw-------");
        d.nodes.get_mut(ENDLESS).expect("just made").size = Some(1_717_986_918);
        d.file(&at(".bashrc"), "# ~/.bashrc\nalias ll='ls -la'\nexport EDITOR=nano\n", NOW.now - 380 * DAY, "-rw-r--r--");
        d.file(&at(".profile"), "# ~/.profile\n[ -f ~/.bashrc ] && . ~/.bashrc\n", NOW.now - 380 * DAY, "-rw-r--r--");
        d.dir(&at(".config"), NOW.now - 90 * DAY);
        d.file(&at(".env"), ENV, NOW.now - 12 * MINUTE - 30, "-rw-------");
        d.file(&at("README.md"), README, NOW.now - 2 * DAY, "-rw-r--r--");
        d.file(&at("nginx.conf"), NGINX, NOW.now - 30 * DAY, "-rw-r--r--");
        d.file(&at("deploy.sh"), DEPLOY, NOW.now - 7 * DAY, "-rwxr-xr-x");
        d.file(&at("logo.png"), logo_png(), NOW.now - 200 * DAY, "-rw-r--r--");
        d.file(&at("shop-2024-10-05.tar.gz"), Vec::new(), NOW.now - 7 * DAY, "-rw-r--r--");
        d.nodes.get_mut(&at("shop-2024-10-05.tar.gz")).expect("just made").size = Some(4_404_019);
        for folder in ["shop", "logs", "backups"] {
            d.nodes.get_mut(&at(folder)).expect("made above").modified = NOW.now - 3 * HOUR;
        }
        d
    }

    /// Any other machine of the demo: a few entries in its home folder.
    pub fn small(home: &str, entries: &[(&str, &str)]) -> Disk {
        let mut d = Disk::empty(home);
        for (i, (name, text)) in entries.iter().enumerate() {
            let (path, modified) = (format!("{home}/{name}"), NOW.now - (i as i64 + 1) * DAY);
            match name.strip_suffix('/') {
                Some(folder) => d.dir(&format!("{home}/{folder}"), modified),
                None => d.file(&path, *text, modified, if name.ends_with(".sh") { "-rwxr-xr-x" } else { "-rw-r--r--" }),
            }
        }
        d
    }

    /// "~" is the home folder; a trailing slash means nothing.
    fn resolve(&self, path: &str) -> String {
        let full = match path {
            "" | "~" => self.home.clone(),
            p => p.strip_prefix("~/").map_or_else(|| p.to_owned(), |rest| paths::join(&self.home, rest)),
        };
        if full.len() > 1 { full.trim_end_matches('/').to_owned() } else { full }
    }

    fn is_dir(&self, path: &str) -> bool {
        self.nodes.get(path).is_some_and(|n| n.kind == EntryKind::Dir)
    }

    /// The paths in `dir`, at any depth.
    fn under(&self, dir: &str) -> Vec<String> {
        let prefix = if dir == "/" { "/".to_owned() } else { format!("{dir}/") };
        // The top of the disk is under nothing, itself included.
        self.nodes.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)).filter(|(k, _)| **k != prefix).map(|(k, _)| k.clone()).collect()
    }

    fn stamp(node: &Node) -> Stamp {
        Stamp { size: node.size.unwrap_or(node.bytes.len() as u64), modified: node.modified, sha256: None }
    }

    pub fn list(&self, path: &str) -> Result<Listing, Failure> {
        let dir = self.resolve(path);
        if !self.is_dir(&dir) {
            return Err(failure(FailureKind::NotFound, "This folder doesn't exist or you can't open it."));
        }
        let entries = self
            .under(&dir)
            .into_iter()
            .filter(|p| paths::parent(p) == dir)
            .map(|p| {
                let n = &self.nodes[&p];
                let size = n.size.unwrap_or(if n.kind == EntryKind::Dir { 4_096 } else { n.bytes.len() as u64 });
                Entry { name: paths::name(&p).to_owned(), kind: n.kind, is_link: n.is_link, size, modified: n.modified, mode: n.mode.to_owned() }
            })
            .collect();
        Ok(Listing { path: dir, entries })
    }

    fn file_at(&self, path: &str) -> Result<&Node, Failure> {
        match self.nodes.get(path) {
            Some(n) if n.kind == EntryKind::File => Ok(n),
            Some(_) => Err(failure(FailureKind::Failed, "This isn't a regular file.")),
            None => Err(not_found()),
        }
    }

    pub fn read(&self, path: &str) -> Result<(TextFile, Stamp), Failure> {
        let node = self.file_at(path)?;
        let size = node.size.unwrap_or(node.bytes.len() as u64);
        let text = if size > MAX_TEXT { None } else { String::from_utf8(node.bytes.clone()).ok().filter(|_| node.size.is_none()) };
        let binary = text.is_none() && size <= MAX_TEXT;
        Ok((TextFile { path: path.to_owned(), content: text, binary, size, lossy: false, crlf: false }, Disk::stamp(node)))
    }

    pub fn read_image(&self, path: &str) -> Result<Vec<u8>, Failure> {
        self.file_at(path).map(|n| n.bytes.clone())
    }

    /// sshee-core's `save_guarded`, in memory: only over the file the stamp was taken of, and
    /// with no stamp only where there's nothing. [`CONTESTED`] has always just been changed
    /// by someone else.
    pub fn save(&mut self, path: &str, text: &str, expected: Option<&Stamp>) -> Result<Stamp, SaveFailure> {
        let Some(node) = self.nodes.get_mut(path) else {
            return match expected {
                Some(_) => Err(SaveFailure::Conflict { now: None }),
                None => {
                    self.file(path, text, NOW.now, "-rw-r--r--");
                    Ok(Disk::stamp(&self.nodes[path]))
                }
            };
        };
        let Some(expected) = expected else { return Err(SaveFailure::Conflict { now: Some(Disk::stamp(node)) }) };
        let knowingly = expected.sha256.as_deref() == Some(SEEN_THEIRS);
        if path == CONTESTED && !knowingly {
            // The colleague saves a moment before you do, every time.
            node.bytes.extend_from_slice(THEIR_LINE.as_bytes());
            node.modified += MINUTE;
            return Err(SaveFailure::Conflict { now: Some(Stamp { sha256: Some(SEEN_THEIRS.into()), ..Disk::stamp(node) }) });
        }
        let now = Disk::stamp(node);
        if (expected.size, expected.modified) != (now.size, now.modified) {
            return Err(SaveFailure::Conflict { now: Some(now) });
        }
        node.bytes = text.as_bytes().to_vec();
        node.size = None;
        node.modified = node.modified.max(NOW.now) + 1;
        Ok(Disk::stamp(node))
    }

    pub fn make_dir(&mut self, path: &str) -> Result<(), Failure> {
        match self.nodes.get(path) {
            Some(n) if n.kind != EntryKind::Dir => Err(failure(FailureKind::Exists, "Something with that name already exists here.")),
            _ => {
                self.dir(path, NOW.now);
                Ok(())
            }
        }
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), Failure> {
        if !self.nodes.contains_key(from) {
            return Err(not_found());
        }
        if self.nodes.contains_key(to) {
            return Err(failure(FailureKind::Exists, "Something with that name already exists here."));
        }
        if !self.is_dir(&paths::parent(to)) || paths::within(to, from) {
            return Err(failure(FailureKind::Failed, "It can't be moved there."));
        }
        let mut moving = self.under(from);
        moving.push(from.to_owned());
        for old in moving {
            let node = self.nodes.remove(&old).expect("listed a moment ago");
            self.nodes.insert(format!("{to}{}", &old[from.len()..]), node);
        }
        Ok(())
    }

    pub fn delete(&mut self, path: &str) -> Result<(), Failure> {
        if path == "/" || path == self.home {
            return Err(failure(FailureKind::Failed, "Files won't delete this folder."));
        }
        self.nodes.remove(path).ok_or_else(not_found)?;
        for inside in self.under(path) {
            self.nodes.remove(&inside);
        }
        Ok(())
    }

    /// "Unpacks" an archive next to itself: a folder named after it, with a few made-up files.
    pub fn extract(&mut self, dir: &str, name: &str) -> Result<String, Failure> {
        self.file_at(&paths::join(dir, name))?;
        if !sshee_app::files::can_unpack(name, false) {
            return Err(failure(FailureKind::Failed, &format!("{name} isn't an archive Files can unpack.")));
        }
        let lower = name.to_lowercase();
        let ending = [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"].into_iter().find(|e| lower.ends_with(e)).map_or_else(|| name.rfind('.').map_or(0, |i| name.len() - i), str::len);
        let base = &name[..name.len() - ending];
        // Nothing is replaced: "shop", then "shop 2".
        let made = (1..).map(|n| if n == 1 { base.to_owned() } else { format!("{base} {n}") }).find(|n| !self.nodes.contains_key(&paths::join(dir, n))).expect("a free name");
        let at = paths::join(dir, &made);
        self.dir(&paths::join(&at, "public"), NOW.now);
        self.file(&paths::join(&at, "VERSION"), format!("{base}\n"), NOW.now, "-rw-r--r--");
        self.file(&paths::join(&at, "public/index.html"), "<!doctype html>\n<title>Shop</title>\n", NOW.now, "-rw-r--r--");
        Ok(made)
    }

    /// An "upload": files of those names appear in `dir`. This computer's files aren't read.
    pub fn upload(&mut self, names: &[String], dir: &str, overwrite: bool) -> Result<(), Failure> {
        if !self.is_dir(dir) {
            return Err(not_found());
        }
        let taken: Vec<&str> = names.iter().map(String::as_str).filter(|n| self.nodes.contains_key(&paths::join(dir, n))).collect();
        if !overwrite && !taken.is_empty() {
            let named = taken.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ");
            return Err(failure(FailureKind::Exists, &format!("{named} already exist{} in this folder.", if taken.len() == 1 { "s" } else { "" })));
        }
        for name in names {
            self.file(&paths::join(dir, name), "Uploaded in the demo.\n", NOW.now, "-rw-r--r--");
        }
        Ok(())
    }
}

/// Every machine's disk, and the downloads that are staged to never finish.
pub struct DemoFiles {
    disks: HashMap<String, Disk>,
    /// The endless downloads still "running", with the tab and machine to tell when stopped.
    endless: HashMap<TransferId, (sshee_app::workbench::TabId, String)>,
}

impl DemoFiles {
    /// `web`, `staging`, `backups`, `db`: the keys of the demo's servers.
    pub fn new(web: &str, staging: &str, backups: &str, db: &str) -> DemoFiles {
        let disks = HashMap::from([
            (web.to_owned(), Disk::web()),
            (staging.to_owned(), {
                let mut d = Disk::small("/home/deploy", &[("shop/", ""), ("README.md", "# shop (staging)\n\nWhat goes live next. Break things here first.\n"), ("deploy.sh", DEPLOY)]);
                // The file the staging worker's log names (`demo.rs`): a Ctrl+click on it opens it.
                d.file("/etc/shop/worker.toml", WORKER, NOW.now - DAY, "-rw-r--r--");
                d
            }),
            (backups.to_owned(), Disk::small("/home/backup", &[("daily/", ""), ("weekly/", ""), ("restore.sh", "#!/bin/sh\n# Restores a dump: ./restore.sh daily/shop-db.sql.gz\nset -eu\necho \"restoring $1\"\n"), ("retention.txt", "daily: 7\nweekly: 8\n")])),
            (db.to_owned(), Disk::small("/var/lib/postgresql", &[("16/", ""), ("migrations/", ""), ("backup.sh", "#!/bin/sh\nset -eu\npg_dump shop | gzip > \"shop-db.sql.gz\"\n")])),
            ("local".to_owned(), Disk::small("/home/dev", &[("Documents/", ""), ("Downloads/", ""), ("Pictures/", ""), ("notes.txt", "Things to do on web-7731:\n- rotate the logs\n- try the new deploy script on staging first\n")])),
        ]);
        DemoFiles { disks, endless: HashMap::new() }
    }

    /// Everything one request is answered with, in order (a transfer: its progress, then its
    /// end). Empty for a request whose answer comes later, when something else happens.
    pub fn answer(&mut self, ask: &Ask) -> Vec<Reply> {
        if let Want::CancelTransfer { id } = ask.want {
            let stopped = failure(FailureKind::Cancelled, "Cancelled.");
            return self.endless.remove(&id).map(|(tab, server)| Reply { tab, server, answer: Answer::Downloaded { id, result: Err(stopped) } }).into_iter().collect();
        }
        let gone = || failure(FailureKind::NotFound, "That server isn't in your list anymore.");
        let Some(disk) = self.disks.get_mut(&ask.server) else {
            // The same shape of answer a server that left the list gets in the program.
            let answer = match ask.want.clone() {
                Want::List { ticket, path } => Answer::Listed { ticket, asked: path, result: Err(gone()) },
                Want::Read { ticket, path } => Answer::Read { ticket, path, result: Err(gone()) },
                Want::ReadImage { ticket, path } => Answer::Image { ticket, path, result: Err(gone()) },
                Want::Save { ticket, path, .. } => Answer::Saved { ticket, path, result: Err(SaveFailure::Failed(gone())) },
                Want::MakeDir { path } => Answer::MadeDir { path, result: Err(gone()) },
                Want::Rename { from, to } => Answer::Renamed { from, to, result: Err(gone()) },
                Want::Delete { dir, names } => Answer::Deleted { dir, deleted: Vec::new(), failed: names.into_iter().map(|n| (n, gone())).collect() },
                Want::Extract { dir, name } => Answer::Extracted { dir, name, result: Err(gone()) },
                Want::Move { from, to, .. } => Answer::Moved { from, to, moved: Vec::new(), taken: Vec::new(), failure: Some(gone()) },
                Want::Download { id, .. } => Answer::Downloaded { id, result: Err(gone()) },
                Want::Upload { id, paths, dir, overwrite } => Answer::Uploaded { id, paths, dir, overwrite, result: Err(gone()) },
                Want::CancelTransfer { .. } => return Vec::new(),
            };
            return vec![ask.reply(answer)];
        };
        let mut answers = Vec::new();
        match ask.want.clone() {
            Want::List { ticket, path } => answers.push(Answer::Listed { ticket, result: disk.list(&path).map(|l| (l, NOW)), asked: path }),
            Want::Read { ticket, path } => answers.push(Answer::Read { ticket, result: disk.read(&path), path }),
            Want::ReadImage { ticket, path } => answers.push(Answer::Image { ticket, result: disk.read_image(&path), path }),
            Want::Save { ticket, path, text, expected } => answers.push(Answer::Saved { ticket, result: disk.save(&path, &text, expected.as_ref()), path }),
            Want::MakeDir { path } => answers.push(Answer::MadeDir { result: disk.make_dir(&path), path }),
            Want::Rename { from, to } => answers.push(Answer::Renamed { result: disk.rename(&from, &to), from, to }),
            Want::Delete { dir, names } => {
                let (mut deleted, mut failed) = (Vec::new(), Vec::new());
                for name in names {
                    match disk.delete(&paths::join(&dir, &name)) {
                        Ok(()) => deleted.push(name),
                        Err(f) => failed.push((name, f)),
                    }
                }
                answers.push(Answer::Deleted { dir, deleted, failed });
            }
            Want::Extract { dir, name } => answers.push(Answer::Extracted { result: disk.extract(&dir, &name), dir, name }),
            Want::Move { from, names, to } => {
                let (mut moved, mut taken, mut stopped) = (Vec::new(), Vec::new(), None);
                for name in names {
                    match disk.rename(&paths::join(&from, &name), &paths::join(&to, &name)) {
                        Ok(()) => moved.push(name),
                        Err(f) if f.kind == FailureKind::Exists => taken.push(name),
                        Err(f) => {
                            stopped = Some(f);
                            break;
                        }
                    }
                }
                answers.push(Answer::Moved { from, to, moved, taken, failure: stopped });
            }
            Want::Download { id, items } => {
                let total: u64 = items.iter().filter_map(|i| disk.nodes.get(&i.path)).map(|n| n.size.unwrap_or(n.bytes.len() as u64)).sum();
                if items.iter().any(|i| i.path == ENDLESS) {
                    // 38% and 1.2 MB/s, for as long as anyone looks: two measures a second
                    // apart on the demo's clock give the card its speed.
                    let done = total * 38 / 100;
                    answers.push(Answer::Progress { id, progress: Progress { done: done - 1_258_291, total, at_ms: 0, ..Progress::default() } });
                    answers.push(Answer::Progress { id, progress: Progress { done, total, at_ms: 1_000, ..Progress::default() } });
                    self.endless.insert(id, (ask.tab, ask.server.clone()));
                } else {
                    let missing = items.iter().find(|i| !disk.nodes.contains_key(&i.path));
                    let result = match missing {
                        Some(_) => Err(not_found()),
                        None => Ok(items.iter().map(|i| paths::join(DOWNLOADS, paths::name(&i.path))).collect()),
                    };
                    answers.push(Answer::Progress { id, progress: Progress { done: total, total, ..Progress::default() } });
                    answers.push(Answer::Downloaded { id, result });
                }
            }
            Want::Upload { id, paths, dir, overwrite } => {
                let names: Vec<String> = paths.iter().map(|p| paths::local_name(&p.to_string_lossy()).to_owned()).collect();
                let result = disk.upload(&names, &dir, overwrite);
                answers.push(Answer::Uploaded { id, paths, dir, overwrite, result });
            }
            Want::CancelTransfer { .. } => {}
        }
        answers.into_iter().map(|a| ask.reply(a)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::files::protocol::Item;
    use sshee_app::files::{glyph, modified_text, Glyph};
    use sshee_app::workbench::TabId;
    use std::path::PathBuf;

    const WEB: &str = "host:demo-web";

    fn demo() -> DemoFiles {
        DemoFiles::new(WEB, "host:demo-staging", "host:demo-backups", "host:demo-db")
    }

    fn one(files: &mut DemoFiles, want: Want) -> Answer {
        let mut replies = files.answer(&Ask { tab: TabId(7), server: WEB.into(), want });
        assert_eq!(replies.len(), 1, "{replies:?}");
        let reply = replies.remove(0);
        assert_eq!((reply.tab, reply.server.as_str()), (TabId(7), WEB));
        reply.answer
    }

    fn names(files: &mut DemoFiles, path: &str) -> Vec<String> {
        let Answer::Listed { result: Ok((l, clock)), .. } = one(files, Want::List { ticket: 1, path: path.into() }) else { panic!("{path}") };
        assert_eq!(clock, NOW);
        l.entries.into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn the_home_folder_has_one_of_everything() {
        let mut files = demo();
        let Answer::Listed { asked, result: Ok((l, _)), .. } = one(&mut files, Want::List { ticket: 1, path: "~".into() }) else { panic!() };
        assert_eq!((asked.as_str(), l.path.as_str()), ("~", "/home/deploy"));
        let shown: Vec<(&str, Glyph)> = l.entries.iter().map(|e| (e.name.as_str(), glyph(e))).collect();
        assert_eq!(
            shown,
            [
                (".bashrc", Glyph::File),
                (".config", Glyph::Folder),
                (".env", Glyph::Code),
                (".profile", Glyph::File),
                ("README.md", Glyph::Text),
                ("backups", Glyph::Folder),
                ("deploy.sh", Glyph::Script),
                ("logo.png", Glyph::Image),
                ("logs", Glyph::Folder),
                ("nginx.conf", Glyph::Code),
                ("shop", Glyph::Folder),
                ("shop-2024-10-05.tar.gz", Glyph::Archive),
            ]
        );
        // The dates column has every way a date reads.
        let dates: Vec<String> = [".env", "shop", "README.md", "deploy.sh", ".bashrc"]
            .iter()
            .map(|n| modified_text(l.entries.iter().find(|e| e.name == *n).unwrap().modified, NOW.now, NOW.utc_offset))
            .collect();
        assert_eq!(dates, ["12 min ago", "Today 11:02", "Oct 10", "Oct 5", "Sep 28, 2023"]);
        assert_eq!(names(&mut files, "~/shop/releases"), ["2024-09-28", "2024-10-05", "2024-10-12", "current", "previous"]);
        assert_eq!(names(&mut files, "/"), ["home"]);
        assert_eq!(names(&mut files, "/home/deploy/logs/"), ["access.log", "deploy.log", "error.log"]);
        let Answer::Listed { result: Err(f), .. } = one(&mut files, Want::List { ticket: 2, path: "/nowhere".into() }) else { panic!() };
        assert_eq!(f.kind, FailureKind::NotFound);
    }

    #[test]
    fn every_machine_has_a_few_entries_and_an_unknown_one_says_so() {
        let mut files = demo();
        for (server, home) in [("host:demo-staging", "/home/deploy"), ("host:demo-backups", "/home/backup"), ("host:demo-db", "/var/lib/postgresql"), ("local", "/home/dev")] {
            let replies = files.answer(&Ask { tab: TabId(1), server: server.into(), want: Want::List { ticket: 1, path: "~".into() } });
            let Answer::Listed { result: Ok((l, _)), .. } = &replies[0].answer else { panic!("{server}") };
            assert_eq!(l.path, home);
            assert!(l.entries.len() >= 3, "{server}");
        }
        let replies = files.answer(&Ask { tab: TabId(1), server: "host:gone".into(), want: Want::List { ticket: 1, path: "~".into() } });
        assert!(matches!(&replies[0].answer, Answer::Listed { result: Err(f), .. } if f.kind == FailureKind::NotFound));
    }

    #[test]
    fn nothing_in_the_demo_is_real() {
        let files = demo();
        for (server, disk) in &files.disks {
            for (path, node) in &disk.nodes {
                let text = String::from_utf8_lossy(&node.bytes);
                for real in ["/root", "ghp_", "sk_live", "BEGIN OPENSSH", "BEGIN RSA"] {
                    assert!(!path.contains(real) && !text.contains(real), "{server} {path}");
                }
                // No address but the documentation range and this machine's own loopback; no email.
                assert!(!text.contains('@'), "{server} {path}");
                for word in text.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|w| w.matches('.').count() == 3 && w.len() >= 7) {
                    assert!(word.starts_with("203.0.113.") || word == "127.0.0.1", "{server} {path}: {word}");
                }
            }
        }
    }

    #[test]
    fn the_picture_is_a_png_rune_can_decode() {
        let png = logo_png();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // IHDR: 48 by 48, 8 bits, RGB; its checksum is the known one for that header.
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..24], &[0, 0, 0, 48, 0, 0, 0, 48]);
        assert_eq!(&png[png.len() - 12..png.len() - 4], &[0, 0, 0, 0, b'I', b'E', b'N', b'D']);
        // The checksum of an empty IEND is the same in every PNG.
        assert_eq!(&png[png.len() - 4..], &[0xae, 0x42, 0x60, 0x82]);
        let mut files = demo();
        let Answer::Image { result: Ok(bytes), .. } = one(&mut files, Want::ReadImage { ticket: 1, path: "/home/deploy/logo.png".into() }) else { panic!() };
        assert_eq!(bytes, png);
        // Asked for as text, it's binary.
        let Answer::Read { result: Ok((file, _)), .. } = one(&mut files, Want::Read { ticket: 1, path: "/home/deploy/logo.png".into() }) else { panic!() };
        assert!(file.binary && file.content.is_none());
    }

    #[test]
    fn a_file_opens_and_saves_and_the_next_save_needs_the_new_stamp() {
        let mut files = demo();
        let path = "/home/deploy/README.md".to_owned();
        let Answer::Read { result: Ok((file, stamp)), .. } = one(&mut files, Want::Read { ticket: 1, path: path.clone() }) else { panic!() };
        assert!(file.content.as_deref().unwrap().starts_with("# shop\n"));
        let Answer::Saved { result: Ok(saved), .. } = one(&mut files, Want::Save { ticket: 2, path: path.clone(), text: "# shop\n\nNew.\n".into(), expected: Some(stamp.clone()) }) else { panic!() };
        assert_ne!(saved, stamp);
        let Answer::Read { result: Ok((file, again)), .. } = one(&mut files, Want::Read { ticket: 3, path: path.clone() }) else { panic!() };
        assert_eq!((file.content.as_deref(), &again), (Some("# shop\n\nNew.\n"), &saved));
        // The old stamp is stale now.
        let Answer::Saved { result: Err(SaveFailure::Conflict { now: Some(now) }), .. } = one(&mut files, Want::Save { ticket: 4, path: path.clone(), text: "x".into(), expected: Some(stamp) }) else { panic!() };
        assert_eq!(now, saved);
        // The listing shows it changed just now.
        let Answer::Listed { result: Ok((l, _)), .. } = one(&mut files, Want::List { ticket: 5, path: "~".into() }) else { panic!() };
        assert_eq!(modified_text(l.entries.iter().find(|e| e.name == "README.md").unwrap().modified, NOW.now, 0), "Just now");
        // A file too big to show, and one that isn't there.
        let Answer::Read { result: Ok((big, _)), .. } = one(&mut files, Want::Read { ticket: 6, path: ENDLESS.into() }) else { panic!() };
        assert!(big.content.is_none() && !big.binary && big.size > MAX_TEXT);
        let Answer::Read { result: Err(f), .. } = one(&mut files, Want::Read { ticket: 7, path: "/home/deploy/none".into() }) else { panic!() };
        assert_eq!(f.kind, FailureKind::NotFound);
    }

    #[test]
    fn the_changelog_always_changed_on_the_server_and_every_way_out_works() {
        let mut files = demo();
        let read = |files: &mut DemoFiles| {
            let Answer::Read { result: Ok((file, stamp)), .. } = one(files, Want::Read { ticket: 1, path: CONTESTED.into() }) else { panic!() };
            (file.content.unwrap(), stamp)
        };
        let save = |files: &mut DemoFiles, path: &str, text: &str, expected: Option<Stamp>| {
            let Answer::Saved { result, .. } = one(files, Want::Save { ticket: 2, path: path.into(), text: text.into(), expected }) else { panic!() };
            result
        };
        let (text, stamp) = read(&mut files);
        let Err(SaveFailure::Conflict { now: Some(now) }) = save(&mut files, CONTESTED, "mine\n", Some(stamp)) else { panic!() };
        // Load theirs: the colleague's line is there.
        let (theirs, fresh) = read(&mut files);
        assert_eq!(theirs, format!("{text}{THEIR_LINE}"));
        // Overwrite, knowingly, with the stamp the conflict gave.
        assert!(save(&mut files, CONTESTED, "mine\n", Some(now)).is_ok());
        assert_eq!(read(&mut files).0, "mine\n");
        // Next time it has changed again: a plain save never goes through.
        assert!(matches!(save(&mut files, CONTESTED, "again\n", Some(fresh)), Err(SaveFailure::Conflict { now: Some(_) })));
        let (_, stamp) = read(&mut files);
        assert!(matches!(save(&mut files, CONTESTED, "again\n", Some(stamp)), Err(SaveFailure::Conflict { now: Some(_) })));
        // A copy beside it: only where there's nothing.
        let copy = "/home/deploy/shop/CHANGELOG (my copy).md";
        assert!(save(&mut files, copy, "mine\n", None).is_ok());
        assert!(matches!(save(&mut files, copy, "mine\n", None), Err(SaveFailure::Conflict { now: Some(_) })));
        assert!(names(&mut files, "~/shop").contains(&"CHANGELOG (my copy).md".to_owned()));
        // A file that's gone.
        assert_eq!(save(&mut files, "/home/deploy/shop/gone.md", "x", Some(Stamp { size: 1, modified: 1, sha256: None })), Err(SaveFailure::Conflict { now: None }));
    }

    #[test]
    fn folders_are_made_renamed_moved_and_deleted() {
        let mut files = demo();
        let Answer::MadeDir { result: Ok(()), .. } = one(&mut files, Want::MakeDir { path: "/home/deploy/drafts".into() }) else { panic!() };
        let Answer::Renamed { result: Ok(()), .. } = one(&mut files, Want::Rename { from: "/home/deploy/shop".into(), to: "/home/deploy/store".into() }) else { panic!() };
        // What was inside came along.
        assert_eq!(names(&mut files, "/home/deploy/store/releases").len(), 5);
        assert!(!names(&mut files, "~").contains(&"shop".to_owned()));
        let Answer::Renamed { result: Err(taken), .. } = one(&mut files, Want::Rename { from: "/home/deploy/drafts".into(), to: "/home/deploy/logs".into() }) else { panic!() };
        assert_eq!(taken.kind, FailureKind::Exists);

        let Answer::MadeDir { result: Ok(()), .. } = one(&mut files, Want::MakeDir { path: "/home/deploy/drafts/logs".into() }) else { panic!() };
        let moving = Want::Move { from: "/home/deploy".into(), names: vec!["README.md".into(), "logs".into(), "nginx.conf".into()], to: "/home/deploy/drafts".into() };
        let Answer::Moved { moved, taken, failure: None, .. } = one(&mut files, moving) else { panic!() };
        assert_eq!((moved, taken), (vec!["README.md".to_owned(), "nginx.conf".to_owned()], vec!["logs".to_owned()]));
        assert_eq!(names(&mut files, "~/drafts"), ["README.md", "logs", "nginx.conf"]);
        // A folder can't go into itself.
        let Answer::Moved { moved, failure: Some(_), .. } = one(&mut files, Want::Move { from: "/home/deploy".into(), names: vec!["drafts".into()], to: "/home/deploy/drafts".into() }) else { panic!() };
        assert!(moved.is_empty());

        let Answer::Deleted { deleted, failed, .. } = one(&mut files, Want::Delete { dir: "/home/deploy".into(), names: vec!["drafts".into(), "nothing".into()] }) else { panic!() };
        assert_eq!((deleted, failed.len()), (vec!["drafts".to_owned()], 1));
        let Answer::Listed { result: Err(_), .. } = one(&mut files, Want::List { ticket: 1, path: "/home/deploy/drafts".into() }) else { panic!() };
        let Answer::Deleted { failed, .. } = one(&mut files, Want::Delete { dir: "/home".into(), names: vec!["deploy".into()] }) else { panic!() };
        assert_eq!(failed.len(), 1, "the home folder stays");
    }

    #[test]
    fn an_archive_unpacks_next_to_itself_under_a_free_name() {
        let mut files = demo();
        let unpack = |files: &mut DemoFiles, name: &str| {
            let Answer::Extracted { result, .. } = one(files, Want::Extract { dir: "/home/deploy".into(), name: name.into() }) else { panic!() };
            result
        };
        assert_eq!(unpack(&mut files, "shop-2024-10-05.tar.gz"), Ok("shop-2024-10-05".into()));
        assert_eq!(names(&mut files, "~/shop-2024-10-05"), ["VERSION", "public"]);
        assert_eq!(unpack(&mut files, "shop-2024-10-05.tar.gz"), Ok("shop-2024-10-05 2".into()));
        assert_eq!(unpack(&mut files, "README.md").unwrap_err().kind, FailureKind::Failed);
        assert_eq!(unpack(&mut files, "none.zip").unwrap_err().kind, FailureKind::NotFound);
    }

    #[test]
    fn an_upload_never_replaces_unasked_and_reads_nothing_of_this_computer() {
        let mut files = demo();
        let paths = vec![PathBuf::from("/does/not/exist/photo.jpg"), PathBuf::from("C:\\Users\\dev\\README.md")];
        let send = |files: &mut DemoFiles, overwrite| {
            let Answer::Uploaded { result, .. } = one(files, Want::Upload { id: TransferId(1), paths: paths.clone(), dir: "/home/deploy".into(), overwrite }) else { panic!() };
            result
        };
        let taken = send(&mut files, false).unwrap_err();
        assert_eq!((taken.kind, taken.message.as_str()), (FailureKind::Exists, "“README.md” already exists in this folder."));
        assert!(!names(&mut files, "~").contains(&"photo.jpg".to_owned()), "nothing went");
        assert_eq!(send(&mut files, true), Ok(()));
        assert!(names(&mut files, "~").contains(&"photo.jpg".to_owned()));
    }

    #[test]
    fn one_download_stays_at_38_percent_until_stopped_and_the_others_finish_at_once() {
        let mut files = demo();
        let ask = |want| Ask { tab: TabId(7), server: WEB.into(), want };
        let quick = files.answer(&ask(Want::Download { id: TransferId(1), items: vec![Item { path: "/home/deploy/README.md".into(), is_dir: false }, Item { path: "/home/deploy/logs".into(), is_dir: true }] }));
        assert!(matches!(&quick[0].answer, Answer::Progress { .. }));
        assert_eq!(quick[1].answer, Answer::Downloaded { id: TransferId(1), result: Ok(vec!["/home/dev/Downloads/README.md".into(), "/home/dev/Downloads/logs".into()]) });

        let endless = files.answer(&ask(Want::Download { id: TransferId(2), items: vec![Item { path: ENDLESS.into(), is_dir: false }] }));
        assert_eq!(endless.len(), 2, "progress, and no end");
        let mut cards = sshee_app::transfers::Transfers::new();
        let id = cards.begin(sshee_app::transfers::Kind::Download, "shop-db.sql.gz", TabId(7), WEB);
        for reply in &endless {
            let Answer::Progress { progress, .. } = &reply.answer else { panic!() };
            cards.progress(id, progress);
        }
        let card = cards.get(id).unwrap().card();
        assert_eq!((card.line.as_str(), card.rate.as_deref()), ("623 MB of 1.6 GB · 38%", Some("1.2 MB/s")));
        // Stopping one that isn't running says nothing; stopping this one ends it, once.
        assert!(files.answer(&ask(Want::CancelTransfer { id: TransferId(1) })).is_empty());
        let stopped = files.answer(&ask(Want::CancelTransfer { id: TransferId(2) }));
        assert!(matches!(&stopped[0].answer, Answer::Downloaded { id: TransferId(2), result: Err(f) } if f.kind == FailureKind::Cancelled));
        assert!(files.answer(&ask(Want::CancelTransfer { id: TransferId(2) })).is_empty());
    }
}
