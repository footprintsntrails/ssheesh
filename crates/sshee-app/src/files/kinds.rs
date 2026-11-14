//! What a file is by its name: the drawing its row gets, whether Files can unpack it, whether
//! it opens as a picture. Ports of `EntryIcon` (FilesPane.tsx), src/lib/archives.ts and
//! src/lib/media.ts of 0.10.

use super::{Entry, EntryKind};

/// The drawing a row gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Folder,
    /// A link whose target is missing.
    BrokenLink,
    Script,
    Code,
    Text,
    Image,
    Video,
    Archive,
    File,
}

const CODE: &[&str] = &[
    "js", "mjs", "cjs", "ts", "tsx", "jsx", "py", "rb", "go", "rs", "java", "kt", "c", "h", "cc", "cpp", "hpp", "php", "json", "yaml", "yml", "toml", "ini", "conf", "cfg",
    "env", "sql", "html", "htm", "css", "scss", "lua", "swift", "vue", "svelte", "xml", "dockerfile",
];
const TEXT: &[&str] = &["md", "txt", "log", "csv", "rst"];
const ARCHIVE: &[&str] = &["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "zst", "deb", "rpm"];
const SCRIPT: &[&str] = &["sh", "bash", "zsh", "fish"];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "avif", "heic", "tif", "tiff"];
const VIDEO: &[&str] = &["mp4", "m4v", "mov", "webm", "ogv", "mkv", "avi"];
/// The pictures Rune decodes itself.
const SHOWN: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];

/// What follows the last dot, in lower case: "tar.GZ" → "gz", ".env" → "env". `None` without a
/// dot.
fn ending(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

fn ends_in(name: &str, list: &[&str]) -> bool {
    ending(name).is_some_and(|e| list.contains(&e.as_str()))
}

/// Which drawing `entry`'s row gets, in 0.10's order: a folder and a broken link by what they
/// are, the rest by name (a script before code, code before text).
pub fn glyph(entry: &Entry) -> Glyph {
    match entry.kind {
        EntryKind::Dir => return Glyph::Folder,
        EntryKind::BrokenLink => return Glyph::BrokenLink,
        EntryKind::File | EntryKind::Other => {}
    }
    let n = entry.name.as_str();
    if ends_in(n, SCRIPT) {
        Glyph::Script
    } else if ends_in(n, CODE) || n == "Dockerfile" || n.starts_with(".env") {
        Glyph::Code
    } else if ends_in(n, TEXT) {
        Glyph::Text
    } else {
        match media_kind(n) {
            Some(MediaKind::Image) => Glyph::Image,
            Some(MediaKind::Video) => Glyph::Video,
            None if ends_in(n, ARCHIVE) => Glyph::Archive,
            None => Glyph::File,
        }
    }
}

/// A picture or a video, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

pub fn media_kind(name: &str) -> Option<MediaKind> {
    if ends_in(name, IMAGE) {
        Some(MediaKind::Image)
    } else if ends_in(name, VIDEO) {
        Some(MediaKind::Video)
    } else {
        None
    }
}

/// What opening a file does in 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// Read as text; the read says if it's binary or too big after all.
    Text,
    /// Read whole and decoded as a picture (png, jpg, gif, webp, bmp, ico).
    Image,
    /// A video, an SVG or a picture Rune can't decode: nothing is read, the screen says it
    /// can't be shown and offers the download.
    Nothing,
}

pub fn opens_as(name: &str) -> Opens {
    if ends_in(name, SHOWN) {
        Opens::Image
    } else if media_kind(name).is_some() {
        Opens::Nothing
    } else {
        Opens::Text
    }
}

/// An archive Files can unpack. sshee-core's `PACKED` (files.rs) must match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Tar,
    /// One file squeezed: .gz, .bz2, .xz, .zst.
    Squeezed,
    SevenZ,
    Rar,
}

const ENDINGS: &[(&str, ArchiveKind)] = &[
    (".tar.gz", ArchiveKind::Tar),
    (".tar.bz2", ArchiveKind::Tar),
    (".tar.xz", ArchiveKind::Tar),
    (".tar.zst", ArchiveKind::Tar),
    (".tgz", ArchiveKind::Tar),
    (".tbz2", ArchiveKind::Tar),
    (".tbz", ArchiveKind::Tar),
    (".txz", ArchiveKind::Tar),
    (".tzst", ArchiveKind::Tar),
    (".tar", ArchiveKind::Tar),
    (".zip", ArchiveKind::Zip),
    (".gz", ArchiveKind::Squeezed),
    (".bz2", ArchiveKind::Squeezed),
    (".xz", ArchiveKind::Squeezed),
    (".zst", ArchiveKind::Squeezed),
    (".7z", ArchiveKind::SevenZ),
    (".rar", ArchiveKind::Rar),
];

/// What kind of archive `name` is, or `None` if Files can't unpack it. A name that is nothing
/// but the ending (".zip") isn't one.
pub fn archive_kind(name: &str) -> Option<ArchiveKind> {
    let lower = name.to_lowercase();
    ENDINGS.iter().find(|(end, _)| lower.len() > end.len() && lower.ends_with(end)).map(|(_, kind)| *kind)
}

/// Whether Files offers to unpack `name`: anywhere a server is, and on this computer too,
/// except that Windows (no sh there) unpacks only .zip and .tar.
pub fn can_unpack(name: &str, on_this_windows: bool) -> bool {
    match archive_kind(name) {
        Some(kind) => !on_this_windows || matches!(kind, ArchiveKind::Zip | ArchiveKind::Tar),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{dir, file};
    use super::*;

    #[test]
    fn a_row_is_drawn_by_what_it_is_then_by_its_name() {
        let of = |name: &str| glyph(&file(name, 1, 1));
        assert_eq!(glyph(&dir("releases.zip")), Glyph::Folder);
        assert_eq!(glyph(&Entry { kind: EntryKind::BrokenLink, is_link: true, ..file("current.sh", 0, 0) }), Glyph::BrokenLink);
        assert_eq!(of("deploy.sh"), Glyph::Script);
        assert_eq!(of("main.RS"), Glyph::Code);
        assert_eq!(of("Dockerfile"), Glyph::Code);
        assert_eq!(of("app.dockerfile"), Glyph::Code);
        assert_eq!(of(".env"), Glyph::Code);
        assert_eq!(of(".env.production"), Glyph::Code);
        assert_eq!(of("nginx.conf"), Glyph::Code);
        assert_eq!(of("README.md"), Glyph::Text);
        assert_eq!(of("access.log"), Glyph::Text);
        assert_eq!(of("logo.png"), Glyph::Image);
        assert_eq!(of("logo.svg"), Glyph::Image);
        assert_eq!(of("intro.mp4"), Glyph::Video);
        assert_eq!(of("backup.tar.gz"), Glyph::Archive);
        assert_eq!(of("tool.deb"), Glyph::Archive);
        assert_eq!(of("LICENSE"), Glyph::File);
        assert_eq!(of("data.bin"), Glyph::File);
        // A socket has no drawing of its own.
        assert_eq!(glyph(&Entry { kind: EntryKind::Other, ..file("agent.sock", 0, 0) }), Glyph::File);
    }

    #[test]
    fn archives_are_told_by_the_end_of_their_name() {
        assert_eq!(archive_kind("site.tar.gz"), Some(ArchiveKind::Tar));
        assert_eq!(archive_kind("SITE.TGZ"), Some(ArchiveKind::Tar));
        assert_eq!(archive_kind("site.tar"), Some(ArchiveKind::Tar));
        assert_eq!(archive_kind("site.zip"), Some(ArchiveKind::Zip));
        assert_eq!(archive_kind("dump.sql.gz"), Some(ArchiveKind::Squeezed));
        assert_eq!(archive_kind("dump.zst"), Some(ArchiveKind::Squeezed));
        assert_eq!(archive_kind("old.7z"), Some(ArchiveKind::SevenZ));
        assert_eq!(archive_kind("old.rar"), Some(ArchiveKind::Rar));
        assert_eq!(archive_kind("notes.txt"), None);
        assert_eq!(archive_kind("tool.deb"), None);
        // Only the ending, no name: 0.10 reads ".tar.gz" as a squeezed ".tar".
        assert_eq!(archive_kind(".zip"), None);
        assert_eq!(archive_kind(".tar.gz"), Some(ArchiveKind::Squeezed));
    }

    #[test]
    fn windows_unpacks_only_zip_and_tar_on_itself() {
        assert!(can_unpack("a.zip", true) && can_unpack("a.tar.gz", true));
        assert!(!can_unpack("a.gz", true) && !can_unpack("a.7z", true) && !can_unpack("a.rar", true));
        assert!(can_unpack("a.gz", false) && can_unpack("a.7z", false) && can_unpack("a.rar", false));
        assert!(!can_unpack("a.txt", false));
    }

    #[test]
    fn pictures_rune_decodes_open_and_other_media_dont() {
        assert_eq!(media_kind("a.JPG"), Some(MediaKind::Image));
        assert_eq!(media_kind("a.heic"), Some(MediaKind::Image));
        assert_eq!(media_kind("a.mkv"), Some(MediaKind::Video));
        assert_eq!(media_kind("a.txt"), None);
        for name in ["a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp", "a.bmp", "a.ico", "A.PNG"] {
            assert_eq!(opens_as(name), Opens::Image, "{name}");
        }
        for name in ["a.svg", "a.avif", "a.heic", "a.tiff", "a.mp4", "a.webm"] {
            assert_eq!(opens_as(name), Opens::Nothing, "{name}");
        }
        for name in ["a.md", "Makefile", ".env", "a.bin", "a.tar.gz"] {
            assert_eq!(opens_as(name), Opens::Text, "{name}");
        }
    }
}
