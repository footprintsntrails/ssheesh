//! Takes what a file says about where it came from out of a copy of it,
//! before the copy goes to a server: where and with what a photo was taken,
//! who wrote a document, in which app and when. The picture or the text
//! itself stays as it was, and so does the file you picked.
//!
//! Only kinds it can take apart safely: JPEG, PNG, WebP, HEIC and AVIF,
//! PDF, Word, Excel and PowerPoint files, and OpenDocument. Anything else
//! goes as it is.

use serde::Serialize;
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Something a file gave away, in the order the app lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trace {
    Location,
    Camera,
    Author,
    Company,
    /// Names on comments and tracked changes.
    Reviewers,
    /// A folder on the computer the file was made on.
    FilePath,
    Dates,
    Software,
    Title,
    Comments,
    Thumbnail,
    CustomProperties,
    /// Data after the end of a picture: a second image, a video clip.
    ExtraData,
    OtherTags,
}

impl Trace {
    /// How the app names it: "Removed location, camera".
    pub fn word(self) -> &'static str {
        match self {
            Trace::Location => "location",
            Trace::Camera => "camera",
            Trace::Author => "author",
            Trace::Company => "company",
            Trace::Reviewers => "reviewer names",
            Trace::FilePath => "file path",
            Trace::Dates => "dates",
            Trace::Software => "software",
            Trace::Title => "title",
            Trace::Comments => "comments",
            Trace::Thumbnail => "thumbnail",
            Trace::CustomProperties => "custom properties",
            Trace::ExtraData => "hidden extra data",
            Trace::OtherTags => "other tags",
        }
    }
}

pub type Found = BTreeSet<Trace>;

/// What cleaning one file came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Scrub {
    /// These came out of the copy.
    Removed(Found),
    /// A kind it cleans, with nothing in it to take out.
    Nothing,
    /// A kind it doesn't clean, or one it couldn't make sense of.
    AsIs,
}

/// What cleaning did to an upload's files so far, for the app to show.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleaning {
    /// Files something came out of.
    pub cleaned: u32,
    /// Files of a kind it cleans that had nothing to take out.
    pub clean: u32,
    /// Files sent as they are.
    pub as_is: u32,
    /// Folders: they go without dates or Mac extras, the files inside as they are.
    pub folders: u32,
    /// What came out, across all files, each once.
    pub removed: Vec<&'static str>,
    #[serde(skip)]
    found: Found,
}

impl Cleaning {
    pub fn add(&mut self, scrub: &Scrub) {
        match scrub {
            Scrub::Removed(found) => {
                self.cleaned += 1;
                self.found.extend(found);
                self.removed = self.found.iter().map(|t| t.word()).collect();
            }
            Scrub::Nothing => self.clean += 1,
            Scrub::AsIs => self.as_is += 1,
        }
    }
}

/// Files bigger than this are read whole to be cleaned; bigger ones go as they are.
const MAX_IN_MEMORY: u64 = 512 * 1024 * 1024;

/// Cleans a copy of the file at `path`; the file itself isn't touched. The
/// copy is there only when something came out; it's deleted when dropped.
pub fn clean_copy(path: &Path) -> io::Result<(Scrub, Option<CleanCopy>)> {
    let mut file = File::open(path)?;
    let mut head = [0u8; 64];
    let n = read_up_to(&mut file, &mut head)?;
    let Some(kind) = sniff(&head[..n]) else { return Ok((Scrub::AsIs, None)) };
    file.seek(SeekFrom::Start(0))?;
    if kind == Kind::Zip {
        return clean_zip(file);
    }
    if file.metadata()?.len() > MAX_IN_MEMORY {
        return Ok((Scrub::AsIs, None));
    }
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    match scrub_kind(kind, data) {
        None => Ok((Scrub::AsIs, None)),
        Some((_, found)) if found.is_empty() => Ok((Scrub::Nothing, None)),
        Some((clean, found)) => {
            let (mut out, mut copy) = CleanCopy::create()?;
            out.write_all(&clean)?;
            out.sync_all()?;
            copy.len = clean.len() as u64;
            Ok((Scrub::Removed(found), Some(copy)))
        }
    }
}

/// Cleans a file held in memory: the clean bytes and what came out (nothing
/// when it had nothing to take out). None for a kind it doesn't clean here
/// (Office files are cleaned from disk), or a file it can't make sense of.
pub fn scrub_bytes(data: Vec<u8>) -> Option<(Vec<u8>, Found)> {
    let kind = sniff(&data[..data.len().min(64)])?;
    scrub_kind(kind, data)
}

fn scrub_kind(kind: Kind, data: Vec<u8>) -> Option<(Vec<u8>, Found)> {
    match kind {
        Kind::Jpeg => scrub_jpeg(&data),
        Kind::Png => scrub_png(&data),
        Kind::Webp => scrub_webp(&data),
        Kind::Heif => scrub_heif(data),
        Kind::Pdf => scrub_pdf(data),
        Kind::Zip => None,
    }
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Jpeg,
    Png,
    Webp,
    Heif,
    Pdf,
    Zip,
}

/// The kind of file, from its first bytes.
fn sniff(head: &[u8]) -> Option<Kind> {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Kind::Jpeg)
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some(Kind::Webp)
    } else if head.len() >= 16 && &head[4..8] == b"ftyp" && is_heif(head) {
        Some(Kind::Heif)
    } else if find(head, b"%PDF-").is_some() {
        Some(Kind::Pdf)
    } else if head.starts_with(b"PK\x03\x04") {
        Some(Kind::Zip)
    } else {
        None
    }
}

/// Whether the `ftyp` box at the start names a HEIF brand (HEIC, AVIF),
/// as the main one or among those it's compatible with.
fn is_heif(head: &[u8]) -> bool {
    const BRANDS: [&[u8; 4]; 11] = [b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"msf1", b"mif2", b"avif", b"avis"];
    let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    let end = size.clamp(16, head.len());
    // The main brand, then (after the minor version) the compatible ones.
    let compatible = (16..end.saturating_sub(3)).step_by(4).map(|at| &head[at..at + 4]);
    std::iter::once(&head[8..12]).chain(compatible).any(|b| BRANDS.iter().any(|x| &x[..] == b))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------- the copy

/// Name of the cleaned copies in the temporary folder.
const COPY_PREFIX: &str = "sshee-clean-";

/// A cleaned copy of a file in the system's temporary folder, readable only
/// by you, deleted when dropped.
pub struct CleanCopy {
    path: PathBuf,
    len: u64,
}

impl CleanCopy {
    fn create() -> io::Result<(File, CleanCopy)> {
        sweep_stale();
        let path = std::env::temp_dir().join(format!("{COPY_PREFIX}{:016x}", crate::files::unguessable()));
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let file = opts.open(&path)?;
        Ok((file, CleanCopy { path, len: 0 }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for CleanCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Removes copies a day old or more, left by an upload the app never got to
/// finish (it was closed, or crashed).
fn sweep_stale() {
    let Ok(dir) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in dir.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(COPY_PREFIX) {
            continue;
        }
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(24 * 3600));
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---------------------------------------------------------------- EXIF and XMP

/// Reads a TIFF structure, as EXIF is.
struct Tiff<'a> {
    d: &'a [u8],
    le: bool,
}

impl<'a> Tiff<'a> {
    fn new(d: &'a [u8]) -> Option<Self> {
        let le = match d.get(..4)? {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => return None,
        };
        Some(Self { d, le })
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.d.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.d.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }

    /// The tags of the directory at `at`, each with where its entry is, and
    /// where the next directory is (0 for none).
    fn ifd(&self, at: usize) -> Option<(Vec<(u16, usize)>, u32)> {
        let n = self.u16(at)? as usize;
        let tags = (0..n).map(|i| at + 2 + i * 12).map(|e| Some((self.u16(e)?, e))).collect::<Option<Vec<_>>>()?;
        Some((tags, self.u32(at + 2 + n * 12).unwrap_or(0)))
    }

    /// The tags of the directory a pointer tag's entry points to.
    fn sub_ifd(&self, entry: usize) -> Vec<u16> {
        self.u32(entry + 8).and_then(|at| self.ifd(at as usize)).map(|(tags, _)| tags.into_iter().map(|(t, _)| t).collect()).unwrap_or_default()
    }
}

/// What EXIF data (a TIFF structure) gives away, and which way up the
/// picture goes (1 when upright or not said). Nothing found when it holds
/// no more than the orientation; other tags when it can't be read.
fn exif_traces(tiff: &[u8]) -> (Found, u16) {
    let mut found = Found::new();
    let mut orientation = 1;
    let read = Tiff::new(tiff).and_then(|t| {
        let (tags, next) = t.ifd(t.u32(4)? as usize)?;
        Some((t, tags, next))
    });
    let Some((t, tags, next)) = read else {
        found.insert(Trace::OtherTags);
        return (found, 1);
    };
    if next != 0 {
        found.insert(Trace::Thumbnail);
    }
    for &(tag, entry) in &tags {
        let trace = match tag {
            0x0112 => {
                orientation = t.u16(entry + 8).filter(|o| (1..=8).contains(o)).unwrap_or(1);
                continue;
            }
            0x010F | 0x0110 => Trace::Camera,
            0x0131 => Trace::Software,
            0x0132 => Trace::Dates,
            0x013B | 0x8298 | 0x9C9D => Trace::Author,
            0x010E | 0x9C9C => Trace::Comments,
            0x9C9B => Trace::Title,
            0x8825 => {
                // GPS: latitude and longitude, not just the version of the format.
                if t.sub_ifd(entry).iter().any(|g| (1..=4).contains(g)) {
                    found.insert(Trace::Location);
                }
                continue;
            }
            0x8769 => {
                for sub in t.sub_ifd(entry) {
                    match sub {
                        0x9003 | 0x9004 | 0x9010 | 0x9011 | 0x9012 => found.insert(Trace::Dates),
                        0xA430 => found.insert(Trace::Author),
                        0x927C | 0xA431 | 0xA433 | 0xA434 | 0xA435 | 0x829A | 0x829D | 0x8827 | 0x920A => found.insert(Trace::Camera),
                        0x9286 => found.insert(Trace::Comments),
                        _ => found.insert(Trace::OtherTags),
                    };
                }
                continue;
            }
            _ => Trace::OtherTags,
        };
        found.insert(trace);
    }
    (found, orientation)
}

/// EXIF that says which way up the picture goes and nothing else, so a
/// photo taken on its side still shows upright.
fn orientation_tiff(orientation: u16) -> Vec<u8> {
    let mut t = b"MM\0*\0\0\0\x08".to_vec();
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&0x0112u16.to_be_bytes());
    t.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&orientation.to_be_bytes());
    t.extend_from_slice(&[0, 0]);
    t.extend_from_slice(&0u32.to_be_bytes());
    t
}

/// An XMP packet with nothing in it.
const EMPTY_XMP: &[u8] = b"<?xpacket begin=\"\xEF\xBB\xBF\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/><?xpacket end=\"w\"?>";

/// What an XMP packet gives away; nothing when it's empty (as this module
/// leaves it), other tags when it's something it can't tell.
fn xmp_traces(xmp: &[u8]) -> Found {
    const MARKS: &[(&[u8], Trace)] = &[
        (b"GPSLatitude", Trace::Location),
        (b"GPSLongitude", Trace::Location),
        (b"LocationCreated", Trace::Location),
        (b"tiff:Make", Trace::Camera),
        (b"tiff:Model", Trace::Camera),
        (b"LensModel", Trace::Camera),
        (b"SerialNumber", Trace::Camera),
        (b"dc:creator", Trace::Author),
        (b"dc:rights", Trace::Author),
        (b"CreateDate", Trace::Dates),
        (b"ModifyDate", Trace::Dates),
        (b"MetadataDate", Trace::Dates),
        (b"DateCreated", Trace::Dates),
        (b"DateTimeOriginal", Trace::Dates),
        (b"CreatorTool", Trace::Software),
        (b"pdf:Producer", Trace::Software),
        (b"softwareAgent", Trace::Software),
        (b"dc:title", Trace::Title),
        (b"dc:description", Trace::Comments),
        (b"UserComment", Trace::Comments),
    ];
    let trimmed = trim(xmp);
    if trimmed.is_empty() || trimmed == EMPTY_XMP {
        return Found::new();
    }
    let mut found: Found = MARKS.iter().filter(|(m, _)| find(xmp, m).is_some()).map(|&(_, t)| t).collect();
    if found.is_empty() {
        found.insert(Trace::OtherTags);
    }
    found
}

fn trim(b: &[u8]) -> &[u8] {
    let blank = |c: &u8| c.is_ascii_whitespace() || *c == 0;
    let start = b.iter().position(|c| !blank(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !blank(c)).map_or(start, |e| e + 1);
    &b[start..end]
}

/// Overwrites `place` with an empty XMP packet padded with spaces, or only
/// spaces when there's no room for one.
fn blank_xmp(place: &mut [u8]) {
    place.fill(b' ');
    if place.len() >= EMPTY_XMP.len() {
        place[..EMPTY_XMP.len()].copy_from_slice(EMPTY_XMP);
    }
}

// ---------------------------------------------------------------- JPEG

fn scrub_jpeg(d: &[u8]) -> Option<(Vec<u8>, Found)> {
    let mut out = Vec::with_capacity(d.len());
    out.extend_from_slice(&d[..2]);
    let mut found = Found::new();
    let mut orientation = 1;
    let mut exif_at = None;
    let mut i = 2;
    loop {
        if *d.get(i)? != 0xFF {
            return None;
        }
        // A marker may be padded with any number of 0xFF.
        while d.get(i) == Some(&0xFF) {
            i += 1;
        }
        let marker = *d.get(i)?;
        i += 1;
        match marker {
            0xD9 => {
                out.extend_from_slice(&[0xFF, 0xD9]);
                // A second picture (a depth map, an HDR layer) or a video clip after the end.
                if d[i..].iter().any(|&b| b != 0) {
                    found.insert(Trace::ExtraData);
                }
                break;
            }
            0x01 | 0xD0..=0xD7 => {
                out.extend_from_slice(&[0xFF, marker]);
                continue;
            }
            0xD8 => return None,
            _ => {}
        }
        let len = u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]) as usize;
        if len < 2 {
            return None;
        }
        let payload = d.get(i + 2..i + len)?;
        i += len;
        // Kept: JFIF, the color profile, and Adobe's color transform. The
        // rest of the APPn segments and comments are an app's own notes.
        let keep = match marker {
            0xE0 if payload.starts_with(b"JFIF\0") => true,
            0xE2 if payload.starts_with(b"ICC_PROFILE\0") => true,
            0xEE if payload.starts_with(b"Adobe") => true,
            0xE1 if payload.starts_with(b"Exif\0\0") => {
                let (traces, o) = exif_traces(&payload[6..]);
                found.extend(traces);
                orientation = o;
                exif_at.get_or_insert(out.len());
                false
            }
            0xE1 if payload.starts_with(b"http://ns.adobe.com/") => {
                let xmp = payload.iter().position(|&b| b == 0).map_or(payload, |z| &payload[z + 1..]);
                let traces = xmp_traces(xmp);
                found.extend(if traces.is_empty() { Found::from([Trace::OtherTags]) } else { traces });
                false
            }
            0xFE => {
                if !trim(payload).is_empty() {
                    found.insert(Trace::Comments);
                }
                false
            }
            0xE0..=0xEF => {
                found.insert(Trace::OtherTags);
                false
            }
            _ => true,
        };
        if keep {
            out.extend_from_slice(&[0xFF, marker]);
            out.extend_from_slice(&d[i - len..i]);
        }
        if marker == 0xDA {
            // The picture itself, up to the next marker that isn't a restart
            // or an escaped 0xFF.
            let start = i;
            loop {
                i += d.get(i..)?.iter().position(|&b| b == 0xFF)?;
                match *d.get(i + 1)? {
                    0x00 | 0xD0..=0xD7 => i += 2,
                    0xFF => i += 1,
                    _ => break,
                }
            }
            out.extend_from_slice(&d[start..i]);
        }
    }
    if orientation != 1 {
        let tiff = orientation_tiff(orientation);
        let mut seg = vec![0xFF, 0xE1];
        seg.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
        seg.extend_from_slice(b"Exif\0\0");
        seg.extend_from_slice(&tiff);
        let at = exif_at.unwrap_or(2);
        out.splice(at..at, seg);
    }
    Some((out, found))
}

// ---------------------------------------------------------------- PNG

fn scrub_png(d: &[u8]) -> Option<(Vec<u8>, Found)> {
    let mut out = d[..8].to_vec();
    let mut found = Found::new();
    let mut orientation = 1;
    let mut exif_at = None;
    let mut i = 8;
    loop {
        let len = u32::from_be_bytes(d.get(i..i + 4)?.try_into().ok()?) as usize;
        let kind: [u8; 4] = d.get(i + 4..i + 8)?.try_into().ok()?;
        let end = i.checked_add(12)?.checked_add(len)?;
        let chunk = d.get(i..end)?;
        let body = &chunk[8..8 + len];
        i = end;
        let keep = match &kind {
            b"tEXt" | b"zTXt" | b"iTXt" => {
                found.extend(png_text_traces(&kind, body));
                false
            }
            b"eXIf" => {
                let (traces, o) = exif_traces(body);
                found.extend(traces);
                orientation = o;
                exif_at.get_or_insert(out.len());
                false
            }
            b"tIME" => {
                found.insert(Trace::Dates);
                false
            }
            // Critical chunks (the picture), and what draws it right:
            // transparency, color, gamma, pixel size, animation.
            _ if kind[0].is_ascii_uppercase() => true,
            b"tRNS" | b"gAMA" | b"cHRM" | b"sRGB" | b"iCCP" | b"sBIT" | b"bKGD" | b"hIST" | b"pHYs" | b"sPLT" | b"acTL" | b"fcTL" | b"fdAT" | b"cICP" | b"mDCV" | b"cLLI" | b"mDCv" | b"cLLi" => true,
            _ => {
                found.insert(Trace::OtherTags);
                false
            }
        };
        if keep {
            out.extend_from_slice(chunk);
        }
        if &kind == b"IEND" {
            if d[i..].iter().any(|&b| b != 0) {
                found.insert(Trace::ExtraData);
            }
            break;
        }
    }
    if orientation != 1 {
        let tiff = orientation_tiff(orientation);
        let mut chunk = (tiff.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"eXIf");
        chunk.extend_from_slice(&tiff);
        chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
        let at = exif_at.unwrap_or(8);
        out.splice(at..at, chunk);
    }
    Some((out, found))
}

/// What a PNG text chunk gives away, by its keyword.
fn png_text_traces(kind: &[u8; 4], body: &[u8]) -> Found {
    let nul = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    let trace = match &body[..nul] {
        b"Author" | b"Copyright" => Trace::Author,
        b"Title" => Trace::Title,
        b"Description" | b"Comment" => Trace::Comments,
        b"Creation Time" | b"date:create" | b"date:modify" | b"date:timestamp" => Trace::Dates,
        b"Software" => Trace::Software,
        b"Source" => Trace::Camera,
        b"XML:com.adobe.xmp" if kind == b"iTXt" => {
            // Keyword, compressed or not, method, language, translated keyword, text.
            let rest = body.get(nul + 1..).unwrap_or_default();
            if rest.first() == Some(&0) {
                let text = rest.get(2..).unwrap_or_default();
                let text = text.splitn(3, |&b| b == 0).nth(2).unwrap_or_default();
                let found = xmp_traces(text);
                if !found.is_empty() {
                    return found;
                }
            }
            Trace::OtherTags
        }
        _ => Trace::OtherTags,
    };
    Found::from([trace])
}

/// CRC-32 as PNG chunks carry it.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

// ---------------------------------------------------------------- WebP

fn scrub_webp(d: &[u8]) -> Option<(Vec<u8>, Found)> {
    let riff = u32::from_le_bytes(d[4..8].try_into().ok()?) as usize;
    let end = riff.checked_add(8)?.min(d.len());
    let mut out = d[..12].to_vec();
    let mut found = Found::new();
    let mut orientation = 1;
    let (mut vp8x_at, mut exif_at) = (None, None);
    let mut i = 12;
    while i + 8 <= end {
        let kind: [u8; 4] = d[i..i + 4].try_into().ok()?;
        let len = u32::from_le_bytes(d[i + 4..i + 8].try_into().ok()?) as usize;
        let body_end = (i + 8).checked_add(len)?;
        let body = d.get(i + 8..body_end)?;
        let next = (body_end + (len & 1)).min(end);
        match &kind {
            b"EXIF" => {
                let (traces, o) = exif_traces(body.strip_prefix(b"Exif\0\0").unwrap_or(body));
                found.extend(traces);
                orientation = o;
                exif_at.get_or_insert(out.len());
            }
            b"XMP " => {
                let traces = xmp_traces(body);
                found.extend(if traces.is_empty() { Found::from([Trace::OtherTags]) } else { traces });
            }
            b"VP8 " | b"VP8L" | b"VP8X" | b"ALPH" | b"ANIM" | b"ANMF" | b"ICCP" => {
                if &kind == b"VP8X" {
                    vp8x_at = Some(out.len());
                }
                out.extend_from_slice(&d[i..next]);
            }
            _ => {
                found.insert(Trace::OtherTags);
            }
        }
        i = next;
    }
    if orientation != 1 {
        let tiff = orientation_tiff(orientation);
        let mut chunk = b"EXIF".to_vec();
        chunk.extend_from_slice(&(tiff.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&tiff);
        if tiff.len() % 2 == 1 {
            chunk.push(0);
        }
        let at = exif_at.unwrap_or(out.len());
        out.splice(at..at, chunk);
    }
    // The extended header says which of EXIF and XMP are there.
    if let Some(flags) = vp8x_at.and_then(|v| out.get_mut(v + 8)) {
        *flags &= !0x04;
        if orientation == 1 {
            *flags &= !0x08;
        }
    }
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Some((out, found))
}

// ---------------------------------------------------------------- HEIF

/// The boxes from `at` to `end`: each one's type, and where its content
/// starts and ends.
fn boxes(d: &[u8], mut at: usize, end: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    while at + 8 <= end {
        let size = u32::from_be_bytes(d[at..at + 4].try_into().unwrap()) as u64;
        let kind: [u8; 4] = d[at + 4..at + 8].try_into().unwrap();
        let (header, size) = match size {
            0 => (8, (end - at) as u64),
            1 => match d.get(at + 8..at + 16) {
                Some(b) => (16, u64::from_be_bytes(b.try_into().unwrap())),
                None => break,
            },
            n => (8, n),
        };
        let Some(box_end) = usize::try_from(size).ok().and_then(|s| at.checked_add(s)).filter(|&e| e <= end && e >= at + header) else { break };
        out.push((kind, at + header, box_end));
        at = box_end;
    }
    out
}

/// A big-endian number of `n` bytes (0, 1, 2, 4 or 8) at `at`.
fn be(d: &[u8], at: usize, n: usize) -> Option<u64> {
    let b = d.get(at..at.checked_add(n)?)?;
    Some(b.iter().fold(0u64, |acc, &x| (acc << 8) | x as u64))
}

/// Blanks the EXIF and XMP items of a HEIC or AVIF in place, so nothing
/// else moves: the EXIF keeps a valid header with no tags, the XMP becomes
/// an empty packet.
fn scrub_heif(mut d: Vec<u8>) -> Option<(Vec<u8>, Found)> {
    let top = boxes(&d, 0, d.len());
    let &(_, ms, me) = top.iter().find(|(k, ..)| k == b"meta")?;
    let inner = boxes(&d, ms + 4, me);
    let get = |name: &[u8; 4]| inner.iter().find(|(k, ..)| k == name).map(|&(_, s, e)| (s, e));
    let (is, ie) = get(b"iinf")?;
    let (ls, le) = get(b"iloc")?;
    let idat = get(b"idat").map(|(s, _)| s);

    // Which items are EXIF, and which XMP.
    let iinf_version = *d.get(is)?;
    let first = is + 4 + if iinf_version == 0 { 2 } else { 4 };
    let mut wanted: Vec<(u64, bool)> = Vec::new();
    for (kind, s, e) in boxes(&d, first, ie) {
        if &kind != b"infe" || *d.get(s)? < 2 {
            continue;
        }
        let id_len = if d[s] == 2 { 2 } else { 4 };
        let id = be(&d, s + 4, id_len)?;
        let at = s + 4 + id_len + 2;
        let item_type = d.get(at..at + 4)?;
        if item_type == b"Exif" {
            wanted.push((id, true));
        } else if item_type == b"mime" {
            // Name, then content type, each ending in a zero byte.
            let rest = d.get(at + 4..e)?;
            let content_type = rest.split(|&b| b == 0).nth(1).unwrap_or_default();
            if content_type == b"application/rdf+xml" {
                wanted.push((id, false));
            }
        }
    }
    if wanted.is_empty() {
        return Some((d, Found::new()));
    }

    // Where they are.
    let version = *d.get(ls)?;
    let sizes = *d.get(ls + 4)?;
    let more = *d.get(ls + 5)?;
    let (offset_size, length_size, base_size) = ((sizes >> 4) as usize, (sizes & 15) as usize, (more >> 4) as usize);
    let index_size = if version >= 1 { (more & 15) as usize } else { 0 };
    let mut at = ls + 6;
    let count_len = if version < 2 { 2 } else { 4 };
    let count = be(&d, at, count_len)?;
    at += count_len;
    let mut found = Found::new();
    for _ in 0..count {
        let id = be(&d, at, count_len)?;
        at += count_len;
        let method = if version >= 1 {
            at += 2;
            be(&d, at - 2, 2)? & 15
        } else {
            0
        };
        at += 2; // data reference index
        let base = be(&d, at, base_size)?;
        at += base_size;
        let extents = be(&d, at, 2)?;
        at += 2;
        let mut ranges = Vec::new();
        for _ in 0..extents {
            at += index_size;
            let offset = be(&d, at, offset_size)?;
            at += offset_size;
            let length = be(&d, at, length_size)?;
            at += length_size;
            let start = match method {
                0 => Some(base + offset),
                1 => idat.map(|s| s as u64 + base + offset),
                _ => None,
            };
            ranges.extend(start.and_then(|s| Some((usize::try_from(s).ok()?, usize::try_from(length).ok()?))));
        }
        if at > le {
            return None;
        }
        let Some(&(_, exif)) = wanted.iter().find(|(w, _)| *w == id) else { continue };
        for (start, len) in ranges {
            let Some(place) = start.checked_add(len).and_then(|end| d.get_mut(start..end)) else { continue };
            if exif {
                found.extend(blank_heif_exif(place));
            } else {
                let traces = xmp_traces(place);
                if !traces.is_empty() {
                    blank_xmp(place);
                    found.extend(traces);
                }
            }
        }
    }
    Some((d, found))
}

/// Blanks a HEIF EXIF item: a 4-byte offset to the TIFF header, then the
/// TIFF data, which becomes a header with no tags.
fn blank_heif_exif(place: &mut [u8]) -> Found {
    let skip = place.get(..4).map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap()) as usize);
    let Some(tiff) = skip.checked_add(4).and_then(|s| place.get_mut(s..)) else {
        place.fill(0);
        return Found::from([Trace::OtherTags]);
    };
    let (found, _) = exif_traces(tiff);
    if found.is_empty() {
        return found;
    }
    tiff.fill(0);
    let empty = b"MM\0*\0\0\0\x08\0\0\0\0\0\0";
    if tiff.len() >= empty.len() {
        tiff[..empty.len()].copy_from_slice(empty);
    }
    found
}

// ---------------------------------------------------------------- PDF

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}

fn is_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// Past whitespace and comments.
fn skip_ws(d: &[u8], mut i: usize) -> usize {
    loop {
        match d.get(i) {
            Some(&b) if is_ws(b) => i += 1,
            Some(b'%') => {
                while i < d.len() && d[i] != b'\n' && d[i] != b'\r' {
                    i += 1;
                }
            }
            _ => return i,
        }
    }
}

/// Where a word (a name, a number, a keyword) starting at `i` ends.
fn word_end(d: &[u8], mut i: usize) -> usize {
    while i < d.len() && !is_ws(d[i]) && !is_delim(d[i]) {
        i += 1;
    }
    i
}

/// Where the PDF value at `i` ends: a dictionary, an array, a string, a
/// name, a number or a keyword. None when it's cut off or nests too deep.
fn value_end(d: &[u8], i: usize, depth: u32) -> Option<usize> {
    if depth > 64 {
        return None;
    }
    match *d.get(i)? {
        b'<' if d.get(i + 1) == Some(&b'<') => dict_entries(d, i, depth + 1).map(|(_, end)| end),
        b'[' => {
            let mut j = i + 1;
            loop {
                j = skip_ws(d, j);
                if *d.get(j)? == b']' {
                    return Some(j + 1);
                }
                j = value_end(d, j, depth + 1)?;
            }
        }
        b'(' => {
            let mut nest = 0usize;
            let mut j = i;
            loop {
                match *d.get(j)? {
                    b'\\' => j += 1,
                    b'(' => nest += 1,
                    b')' => {
                        nest -= 1;
                        if nest == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        b'<' => Some(i + d[i..].iter().position(|&b| b == b'>')? + 1),
        b'/' => Some(word_end(d, i + 1)),
        b if is_delim(b) => None,
        _ => Some(word_end(d, i)),
    }
}

/// Where a dictionary entry's key and its value start and end.
type Entry = ((usize, usize), (usize, usize));

/// The dictionary at `i` (which starts with `<<`): where each key and its
/// value are, and where the dictionary ends. A reference ("12 0 R") counts
/// as one value.
fn dict_entries(d: &[u8], i: usize, depth: u32) -> Option<(Vec<Entry>, usize)> {
    let mut j = i + 2;
    let mut entries = Vec::new();
    loop {
        j = skip_ws(d, j);
        if d.get(j..j + 2)? == b">>" {
            return Some((entries, j + 2));
        }
        if d[j] != b'/' {
            return None;
        }
        let key_end = word_end(d, j + 1);
        let vs = skip_ws(d, key_end);
        let mut ve = value_end(d, vs, depth)?;
        if d[vs].is_ascii_digit() {
            let g = skip_ws(d, ve);
            if d.get(g).is_some_and(u8::is_ascii_digit) {
                let r = skip_ws(d, word_end(d, g));
                if d.get(r) == Some(&b'R') && word_end(d, r) == r + 1 {
                    ve = r + 1;
                }
            }
        }
        entries.push(((j, key_end), (vs, ve)));
        j = ve;
    }
}

/// A dictionary's value for `key` ("/Info").
fn entry<'a>(d: &'a [u8], entries: &[Entry], key: &[u8]) -> Option<&'a [u8]> {
    entries.iter().find(|((ks, ke), _)| &d[*ks..*ke] == key).map(|&(_, (vs, ve))| &d[vs..ve])
}

/// "12 0 R" as (12, 0).
fn parse_ref(v: &[u8]) -> Option<(u32, u16)> {
    let s = std::str::from_utf8(v).ok()?;
    let mut parts = s.split_ascii_whitespace();
    let (num, gen) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    (parts.next()? == "R").then_some((num, gen))
}

struct PdfObject {
    /// Where "12 0 obj" starts.
    at: usize,
    num: u32,
    gen: u16,
    /// Where its dictionary starts and ends, when it is one.
    dict: Option<(usize, usize)>,
    /// Where its stream's data starts and ends, when it has one.
    stream: Option<(usize, usize)>,
}

/// The "12 0" before an "obj" at `at`, and where it starts.
fn obj_header(d: &[u8], at: usize) -> Option<(u32, u16, usize)> {
    let digits_back = |end: usize| {
        let start = d[..end].iter().rposition(|b| !b.is_ascii_digit()).map_or(0, |p| p + 1);
        (start < end && end - start <= 10).then_some(start)
    };
    let mut j = at;
    if j == 0 || !is_ws(d[j - 1]) {
        return None;
    }
    while j > 0 && is_ws(d[j - 1]) {
        j -= 1;
    }
    let gs = digits_back(j)?;
    let gen = std::str::from_utf8(&d[gs..j]).ok()?.parse().ok()?;
    j = gs;
    if j == 0 || !is_ws(d[j - 1]) {
        return None;
    }
    while j > 0 && is_ws(d[j - 1]) {
        j -= 1;
    }
    let ns = digits_back(j)?;
    if ns > 0 && !is_ws(d[ns - 1]) && !is_delim(d[ns - 1]) {
        return None;
    }
    Some((std::str::from_utf8(&d[ns..j]).ok()?.parse().ok()?, gen, ns))
}

/// Every object in the file, in order, stepping over stream data so nothing
/// inside a stream is taken for an object.
fn pdf_objects(d: &[u8]) -> Vec<PdfObject> {
    let mut objects = Vec::new();
    let mut i = 0;
    while let Some(p) = find(&d[i..], b"obj") {
        let at = i + p;
        i = at + 3;
        if d.get(at + 3).is_some_and(|&b| !is_ws(b) && !is_delim(b)) {
            continue;
        }
        let Some((num, gen, start)) = obj_header(d, at) else { continue };
        let mut obj = PdfObject { at: start, num, gen, dict: None, stream: None };
        let vs = skip_ws(d, at + 3);
        if d.get(vs..vs + 2) == Some(&b"<<"[..]) {
            if let Some((entries, end)) = dict_entries(d, vs, 0) {
                obj.dict = Some((vs, end));
                i = end;
                let s = skip_ws(d, end);
                if d[s..].starts_with(b"stream") {
                    let mut ds = s + 6;
                    if d.get(ds) == Some(&b'\r') {
                        ds += 1;
                    }
                    if d.get(ds) == Some(&b'\n') {
                        ds += 1;
                    }
                    // The length it gives, when that ends at "endstream";
                    // else wherever "endstream" is.
                    let given = entry(d, &entries, b"/Length")
                        .and_then(|v| std::str::from_utf8(v).ok()?.parse::<usize>().ok())
                        .and_then(|n| ds.checked_add(n))
                        .filter(|&e| e <= d.len() && d[skip_ws(d, e)..].starts_with(b"endstream"));
                    let de = given.or_else(|| {
                        let e = ds + find(&d[ds..], b"endstream")?;
                        Some(if d[..e].ends_with(b"\r\n") { e - 2 } else if d[..e].ends_with(b"\n") || d[..e].ends_with(b"\r") { e - 1 } else { e })
                    });
                    if let Some(de) = de {
                        obj.stream = Some((ds, de));
                        i = de;
                    }
                }
            }
        }
        objects.push(obj);
    }
    objects
}

/// Blanks the document information (author, title, app, dates) and every
/// XMP stream of a PDF in place: same length, so every offset in the file
/// still points where it did.
fn scrub_pdf(mut d: Vec<u8>) -> Option<(Vec<u8>, Found)> {
    let objects = pdf_objects(&d);
    let mut found = Found::new();

    // The document information named by each trailer (one per update), or
    // by a cross-reference stream.
    let mut info: Vec<(u32, u16, usize, usize)> = Vec::new();
    let in_stream = |at: usize| objects.iter().any(|o| o.stream.is_some_and(|(s, e)| s <= at && at < e));
    let mut i = 0;
    while let Some(p) = find(&d[i..], b"trailer") {
        let at = i + p;
        i = at + 7;
        let ds = skip_ws(&d, i);
        if in_stream(at) || d.get(ds..ds + 2) != Some(&b"<<"[..]) {
            continue;
        }
        if let Some((entries, _)) = dict_entries(&d, ds, 0) {
            info.extend(entries.iter().filter(|((ks, ke), _)| &d[*ks..*ke] == b"/Info").filter_map(|&((ks, _), (vs, ve))| {
                let (n, g) = parse_ref(&d[vs..ve])?;
                Some((n, g, ks, ve))
            }));
        }
    }
    for o in &objects {
        let Some((ds, _)) = o.dict else { continue };
        let Some((entries, _)) = dict_entries(&d, ds, 0) else { continue };
        if entry(&d, &entries, b"/Type") == Some(&b"/XRef"[..]) {
            info.extend(entries.iter().filter(|((ks, ke), _)| &d[*ks..*ke] == b"/Info").filter_map(|&((ks, _), (vs, ve))| {
                let (n, g) = parse_ref(&d[vs..ve])?;
                Some((n, g, ks, ve))
            }));
        }
    }

    for &(num, gen, ks, ve) in &info {
        let mut wiped = false;
        for o in objects.iter().filter(|o| o.num == num && o.gen == gen) {
            let Some((ds, de)) = o.dict else { continue };
            let Some((entries, _)) = dict_entries(&d, ds, 0) else { continue };
            for ((ks, ke), _) in &entries {
                found.insert(match &d[*ks..*ke] {
                    b"/Author" => Trace::Author,
                    b"/Creator" | b"/Producer" => Trace::Software,
                    b"/CreationDate" | b"/ModDate" => Trace::Dates,
                    b"/Title" => Trace::Title,
                    b"/Subject" | b"/Keywords" => Trace::Comments,
                    b"/Company" => Trace::Company,
                    _ => Trace::OtherTags,
                });
            }
            d[ds + 2..de - 2].fill(b' ');
            wiped = true;
        }
        // Not an object of its own: packed into a compressed object stream.
        // Unhooking it keeps readers from showing it.
        if !wiped {
            d[ks..ve].fill(b' ');
            found.insert(Trace::OtherTags);
        }
    }

    // Comments before the first object, where some apps sign the file ("%
    // created by …"). The version line, and the one of high bytes that
    // marks the file as binary, stay.
    let head_end = objects.first().map_or(0, |o| o.at);
    let mut line = 0;
    while line < head_end {
        let end = d[line..head_end].iter().position(|&b| b == b'\n' || b == b'\r').map_or(head_end, |p| line + p);
        let text = &d[line..end];
        let marker = text.len() > 1 && text[1..].iter().all(|&b| b >= 0x80);
        if text.first() == Some(&b'%') && !text.starts_with(b"%PDF-") && !marker && !trim(&text[1..]).is_empty() {
            d[line + 1..end].fill(b' ');
            found.insert(Trace::Software);
        }
        line = end + 1;
    }

    for o in &objects {
        let (Some((ds, _)), Some((ss, se))) = (o.dict, o.stream) else { continue };
        let Some((entries, _)) = dict_entries(&d, ds, 0) else { continue };
        if entry(&d, &entries, b"/Type") != Some(&b"/Metadata"[..]) || entry(&d, &entries, b"/Subtype") != Some(&b"/XML"[..]) {
            continue;
        }
        let packed = entry(&d, &entries, b"/Filter").is_some();
        let traces = if packed { Found::from([Trace::OtherTags]) } else { xmp_traces(&d[ss..se]) };
        if traces.is_empty() {
            continue;
        }
        found.extend(traces);
        // Compressed XMP is left as plain spaces, so it says so.
        for ((ks, ke), (_, ve)) in entries {
            if matches!(&d[ks..ke], b"/Filter" | b"/DecodeParms") {
                d[ks..ve].fill(b' ');
            }
        }
        blank_xmp(&mut d[ss..se]);
    }
    Some((d, found))
}

// ---------------------------------------------------------------- Office

#[derive(Clone, Copy, PartialEq, Eq)]
enum Package {
    /// Word, Excel, PowerPoint.
    Ooxml,
    /// LibreOffice and other OpenDocument files.
    Odf,
}

/// Biggest part of an Office file it reads to clean.
const MAX_PART: u64 = 256 * 1024 * 1024;

/// Cleans a copy of an Office file: its properties (author, company, app,
/// dates, title), names on comments and tracked changes, and the photos in
/// it. Parts it doesn't change are copied over as they are, compressed.
fn clean_zip(file: File) -> io::Result<(Scrub, Option<CleanCopy>)> {
    let Ok(mut zip) = ZipArchive::new(file) else { return Ok((Scrub::AsIs, None)) };
    let names: Vec<String> = (0..zip.len()).map(|i| zip.name_for_index(i).and_then(Result::ok).map(String::from).unwrap_or_default()).collect();
    let package = if names.iter().any(|n| n == "[Content_Types].xml") {
        Package::Ooxml
    } else if names.iter().any(|n| n == "mimetype") && zip_text(&mut zip, "mimetype").is_some_and(|m| m.starts_with("application/vnd.oasis.opendocument")) {
        Package::Odf
    } else {
        return Ok((Scrub::AsIs, None));
    };

    let mut found = Found::new();
    let mut changed: Vec<Option<Vec<u8>>> = vec![None; names.len()];
    for (i, name) in names.iter().enumerate() {
        let Some(part) = part_kind(package, name) else { continue };
        // One it can't read (an unusual compression) goes over as it is.
        let Ok(mut entry) = zip.by_index(i) else { continue };
        if entry.size() > MAX_PART {
            continue;
        }
        let mut data = Vec::new();
        if entry.read_to_end(&mut data).is_err() {
            continue;
        }
        drop(entry);
        if let Some((clean, traces)) = clean_part(part, data) {
            if !traces.is_empty() {
                found.extend(traces);
                changed[i] = Some(clean);
            }
        }
    }
    if found.is_empty() {
        return Ok((Scrub::Nothing, None));
    }

    let (out, mut copy) = CleanCopy::create()?;
    let mut w = ZipWriter::new(out);
    for (i, change) in changed.into_iter().enumerate() {
        // Every part dated the same, as Office itself does.
        match change {
            Some(data) => {
                let stored = zip.by_index_raw(i)?.compression() == CompressionMethod::Stored;
                let method = if stored { CompressionMethod::Stored } else { CompressionMethod::Deflated };
                let opts = SimpleFileOptions::default().compression_method(method).last_modified_time(DateTime::default()).large_file(data.len() as u64 >= u32::MAX as u64);
                w.start_file(names[i].as_str(), opts)?;
                w.write_all(&data)?;
            }
            None => w.raw_copy_file_touch(zip.by_index_raw(i)?, DateTime::default(), None)?,
        }
    }
    let mut out = w.finish()?;
    out.flush()?;
    out.sync_all()?;
    copy.len = out.metadata()?.len();
    Ok((Scrub::Removed(found), Some(copy)))
}

fn zip_text(zip: &mut ZipArchive<File>, name: &str) -> Option<String> {
    let mut s = String::new();
    zip.by_name(name).ok()?.take(4096).read_to_string(&mut s).ok()?;
    Some(s)
}

#[derive(Clone, Copy)]
enum Part {
    /// docProps/core.xml: author, title, dates.
    Core,
    /// docProps/app.xml: company, manager, app, template, editing time.
    App,
    /// docProps/custom.xml.
    Custom,
    /// Word's own parts: names and dates on comments and tracked changes.
    WordXml,
    /// Excel's list of comment authors.
    ExcelComments,
    /// Excel's and PowerPoint's people who commented.
    People,
    /// xl/workbook.xml, which may name the folder it was saved in.
    Workbook,
    /// OpenDocument's meta.xml.
    OdfMeta,
    /// OpenDocument's content and styles: names on comments and changes.
    OdfXml,
    /// A picture inside.
    Media,
}

fn part_kind(package: Package, name: &str) -> Option<Part> {
    let in_folder = |dir: &str| name.strip_prefix(dir).is_some_and(|rest| !rest.contains('/'));
    let picture = name.contains("/media/") || name.starts_with("Pictures/") || name.starts_with("Thumbnails/") || name == "docProps/thumbnail.jpeg";
    Some(match package {
        Package::Ooxml => match name {
            "docProps/core.xml" => Part::Core,
            "docProps/app.xml" => Part::App,
            "docProps/custom.xml" => Part::Custom,
            "xl/workbook.xml" => Part::Workbook,
            "ppt/commentAuthors.xml" | "ppt/authors.xml" => Part::People,
            _ if in_folder("xl/persons/") => Part::People,
            _ if in_folder("xl/") && name.starts_with("xl/comments") && name.ends_with(".xml") => Part::ExcelComments,
            _ if in_folder("word/") && name.ends_with(".xml") => Part::WordXml,
            _ if picture => Part::Media,
            _ => return None,
        },
        Package::Odf => match name {
            "meta.xml" => Part::OdfMeta,
            "content.xml" | "styles.xml" => Part::OdfXml,
            _ if picture => Part::Media,
            _ => return None,
        },
    })
}

const EMPTY_CORE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"/>";
const EMPTY_APP: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"/>";
const EMPTY_CUSTOM: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"/>";

/// A part cleaned, and what came out of it (nothing: leave it as it is).
fn clean_part(part: Part, data: Vec<u8>) -> Option<(Vec<u8>, Found)> {
    if let Part::Media = part {
        return scrub_bytes(data);
    }
    let xml = String::from_utf8(data).ok()?;
    let mut found = Found::new();
    let clean = match part {
        Part::Core => {
            for name in filled(&xml) {
                found.insert(match name {
                    "creator" | "lastModifiedBy" => Trace::Author,
                    "created" | "modified" | "lastPrinted" => Trace::Dates,
                    "title" => Trace::Title,
                    "description" => Trace::Comments,
                    _ => Trace::OtherTags,
                });
            }
            EMPTY_CORE.to_string()
        }
        Part::App => {
            for name in filled(&xml) {
                let trace = match name {
                    "Company" => Trace::Company,
                    "Manager" => Trace::Author,
                    "Application" | "AppVersion" | "Template" => Trace::Software,
                    "TotalTime" => Trace::Dates,
                    "HyperlinkBase" => Trace::FilePath,
                    // Page and word counts, sheet names: about the content.
                    _ => continue,
                };
                found.insert(trace);
            }
            EMPTY_APP.to_string()
        }
        Part::Custom => {
            if xml.contains("<property") {
                found.insert(Trace::CustomProperties);
            }
            EMPTY_CUSTOM.to_string()
        }
        Part::WordXml => rewrite_attrs(&xml, |_, attr, value| match attr {
            "w:author" | "w:initials" | "w15:author" | "w15:userId" if !value.is_empty() => {
                found.insert(Trace::Reviewers);
                Attr::Blank
            }
            "w:date" | "w16cex:dateUtc" => {
                found.insert(Trace::Dates);
                Attr::Drop
            }
            _ => Attr::Keep,
        })?,
        Part::People => rewrite_attrs(&xml, |_, attr, value| match attr {
            "name" | "initials" | "userId" | "displayName" if !value.is_empty() => {
                found.insert(Trace::Reviewers);
                Attr::Blank
            }
            _ => Attr::Keep,
        })?,
        Part::Workbook => rewrite_attrs(&xml, |tag, attr, value| {
            if tag.ends_with(":absPath") && attr == "url" && !value.is_empty() {
                found.insert(Trace::FilePath);
                Attr::Blank
            } else {
                Attr::Keep
            }
        })?,
        Part::ExcelComments => {
            let clean = blank_texts(&xml, "author")?;
            found.insert(Trace::Reviewers);
            clean
        }
        Part::OdfMeta => {
            for name in filled(&xml) {
                found.insert(match name {
                    "initial-creator" | "creator" => Trace::Author,
                    "creation-date" | "date" | "print-date" | "editing-duration" => Trace::Dates,
                    "generator" => Trace::Software,
                    "title" => Trace::Title,
                    "description" => Trace::Comments,
                    "user-defined" => Trace::CustomProperties,
                    _ => Trace::OtherTags,
                });
            }
            if xml.contains("<meta:template") {
                found.insert(Trace::FilePath);
            }
            let version = attr_value(&xml, "office:version").unwrap_or("1.2");
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" office:version=\"{version}\"><office:meta/></office:document-meta>")
        }
        Part::OdfXml => {
            let clean = blank_texts(&xml, "dc:creator")?;
            found.insert(Trace::Reviewers);
            clean
        }
        Part::Media => unreachable!(),
    };
    Some((clean.into_bytes(), found))
}

/// Local names of the elements in `xml` that hold text of their own:
/// `<dc:creator>Ann</dc:creator>` gives "creator".
fn filled(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        if rest.starts_with(['/', '?', '!']) {
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        let after = &rest[gt + 1..];
        let text = &after[..after.find('<').unwrap_or(after.len())];
        if !tag.ends_with('/') && !text.trim().is_empty() && after[text.len()..].starts_with("</") {
            let qname = tag.split(|c: char| c.is_whitespace()).next().unwrap_or_default();
            out.push(qname.rsplit(':').next().unwrap_or(qname));
        }
    }
    out
}

/// The value of the first `name="…"` attribute in `xml`.
fn attr_value<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let at = xml.find(&format!("{name}=\""))? + name.len() + 2;
    Some(&xml[at..at + xml[at..].find('"')?])
}

enum Attr {
    Keep,
    Blank,
    Drop,
}

/// `xml` with attributes emptied or dropped as `what` says for each: given
/// the tag's name, the attribute's name (both with their prefix) and its
/// value. None when nothing changed.
fn rewrite_attrs(xml: &str, mut what: impl FnMut(&str, &str, &str) -> Attr) -> Option<String> {
    let mut out = String::with_capacity(xml.len());
    let mut changed = false;
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..=lt]);
        rest = &rest[lt + 1..];
        if rest.starts_with(['/', '?', '!']) {
            continue;
        }
        let name_end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
        let tag = &rest[..name_end];
        out.push_str(tag);
        rest = &rest[name_end..];
        // Attributes, up to the end of the tag.
        loop {
            let ws = rest.len() - rest.trim_start().len();
            let (space, after) = rest.split_at(ws);
            if after.is_empty() || after.starts_with(['>', '/']) {
                break;
            }
            let Some(eq) = after.find('=') else { break };
            let attr = after[..eq].trim_end();
            let quoted = after[eq + 1..].trim_start();
            let Some(q) = quoted.chars().next().filter(|&c| c == '"' || c == '\'') else { break };
            let Some(close) = quoted[1..].find(q) else { break };
            let value = &quoted[1..1 + close];
            let whole = after.len() - quoted.len() + close + 2;
            match what(tag, attr, value) {
                Attr::Keep => {
                    out.push_str(space);
                    out.push_str(&after[..whole]);
                }
                Attr::Blank => {
                    out.push_str(space);
                    out.push_str(attr);
                    out.push('=');
                    out.push(q);
                    out.push(q);
                    changed = true;
                }
                Attr::Drop => changed = true,
            }
            rest = &after[whole..];
        }
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// `xml` with the text of every `<name>…</name>` emptied. None when none
/// had any.
fn blank_texts(xml: &str, name: &str) -> Option<String> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let mut out = String::with_capacity(xml.len());
    let mut changed = false;
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let start = at + open.len();
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(&close) else { break };
        changed |= !rest[..end].trim().is_empty();
        rest = &rest[end..];
    }
    out.push_str(rest);
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(found: &Found) -> Vec<&'static str> {
        found.iter().map(|t| t.word()).collect()
    }

    /// EXIF (as a TIFF, little-endian) with a camera, an orientation, a date and a GPS position.
    fn exif_with_gps(orientation: u16) -> Vec<u8> {
        let mut t = b"II*\0\x08\0\0\0".to_vec();
        // IFD0 at 8: Make, Orientation, DateTime, GPS pointer.
        let entries: [(u16, u16, u32, u32); 4] = [(0x010F, 2, 6, 62), (0x0112, 3, 1, orientation as u32), (0x0132, 2, 20, 68), (0x8825, 4, 1, 88)];
        t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, typ, count, value) in entries {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&typ.to_le_bytes());
            t.extend_from_slice(&count.to_le_bytes());
            t.extend_from_slice(&value.to_le_bytes());
        }
        t.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(t.len(), 62);
        t.extend_from_slice(b"Apple\0");
        t.extend_from_slice(b"2024:06:01 12:00:00\0");
        assert_eq!(t.len(), 88);
        // GPS IFD at 88: latitude ref and longitude ref.
        t.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(1u16, *b"N\0\0\0"), (3u16, *b"E\0\0\0")] {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&2u16.to_le_bytes());
            t.extend_from_slice(&2u32.to_le_bytes());
            t.extend_from_slice(&value);
        }
        t.extend_from_slice(&0u32.to_le_bytes());
        t
    }

    fn jpeg_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    /// A tiny JPEG-shaped file: the segments, a scan with an escaped 0xFF and a restart, the end.
    fn jpeg(segments: &[Vec<u8>], after_end: &[u8]) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8];
        for s in segments {
            j.extend_from_slice(s);
        }
        j.extend_from_slice(&jpeg_segment(0xDB, &[0; 65]));
        j.extend_from_slice(&jpeg_segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        j.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]);
        j.extend_from_slice(&[0xFF, 0xD9]);
        j.extend_from_slice(after_end);
        j
    }

    #[test]
    fn a_jpeg_loses_its_gps_camera_and_date_but_stays_upright() {
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend_from_slice(&exif_with_gps(6));
        let xmp = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta><rdf:Description xmp:CreatorTool=\"Photos\"/></x:xmpmeta>";
        let jfif = jpeg_segment(0xE0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0");
        let icc = jpeg_segment(0xE2, b"ICC_PROFILE\0\x01\x01profile");
        let input = jpeg(&[jfif.clone(), jpeg_segment(0xE1, &exif), jpeg_segment(0xE1, xmp), icc.clone(), jpeg_segment(0xFE, b"made by Ann")], b"MPF second picture");
        let (out, found) = scrub_bytes(input.clone()).unwrap();
        assert_eq!(words(&found), ["location", "camera", "dates", "software", "comments", "hidden extra data"]);
        // The picture and what draws it right are all still there, in order.
        let scan = [0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56, 0xFF, 0xD9];
        assert!(out.ends_with(&scan));
        assert!(find(&out, &jfif).is_some() && find(&out, &icc).is_some());
        // Gone: the camera, the place, the comment, the extra picture.
        for gone in [&b"Apple"[..], b"2024:06:01", b"Photos", b"made by Ann", b"MPF"] {
            assert!(find(&out, gone).is_none(), "{}", String::from_utf8_lossy(gone));
        }
        // What's left of the EXIF is the orientation alone, so the photo stays upright.
        let at = find(&out, b"Exif\0\0").unwrap();
        let (left, orientation) = exif_traces(&out[at + 6..]);
        assert!(left.is_empty());
        assert_eq!(orientation, 6);
        // Cleaning it again finds nothing: the clean copy is as clean as it gets.
        assert!(scrub_bytes(out).unwrap().1.is_empty());
        // The original is untouched by all of this.
        assert!(find(&input, b"Apple").is_some());
    }

    #[test]
    fn a_plain_jpeg_has_nothing_to_remove() {
        let input = jpeg(&[jpeg_segment(0xE0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0")], b"");
        let (out, found) = scrub_bytes(input.clone()).unwrap();
        assert!(found.is_empty());
        assert_eq!(out, input);
    }

    fn png_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut c = (body.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(body);
        c.extend_from_slice(&crc32(&c[4..]).to_be_bytes());
        c
    }

    #[test]
    fn a_png_loses_its_text_chunks() {
        let ihdr = png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        let phys = png_chunk(b"pHYs", &[0, 0, 11, 19, 0, 0, 11, 19, 1]);
        let idat = png_chunk(b"IDAT", b"pixels");
        let iend = png_chunk(b"IEND", b"");
        let mut input = b"\x89PNG\r\n\x1a\n".to_vec();
        for c in [&ihdr, &png_chunk(b"tEXt", b"Author\0Ann Smith"), &png_chunk(b"tEXt", b"Software\0GIMP 2.10"), &phys, &png_chunk(b"tIME", &[7, 232, 6, 1, 12, 0, 0]), &idat, &iend] {
            input.extend_from_slice(c);
        }
        let (out, found) = scrub_bytes(input).unwrap();
        assert_eq!(words(&found), ["author", "dates", "software"]);
        let mut want = b"\x89PNG\r\n\x1a\n".to_vec();
        for c in [&ihdr, &phys, &idat, &iend] {
            want.extend_from_slice(c);
        }
        assert_eq!(out, want);
        // The CRC is the standard one, so a hand-made eXIf chunk checks out.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn a_webp_loses_exif_and_xmp_and_says_so_in_its_header() {
        let exif = exif_with_gps(1);
        let chunk = |kind: &[u8; 4], body: &[u8]| {
            let mut c = kind.to_vec();
            c.extend_from_slice(&(body.len() as u32).to_le_bytes());
            c.extend_from_slice(body);
            if body.len() % 2 == 1 {
                c.push(0);
            }
            c
        };
        let vp8x = chunk(b"VP8X", &[0x0C, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let image = chunk(b"VP8L", b"pixels!");
        let body = [vp8x, image.clone(), chunk(b"EXIF", &exif), chunk(b"XMP ", b"<x:xmpmeta/>")].concat();
        let mut input = b"RIFF".to_vec();
        input.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        input.extend_from_slice(b"WEBP");
        input.extend_from_slice(&body);
        let (out, found) = scrub_bytes(input).unwrap();
        assert_eq!(words(&found), ["location", "camera", "dates", "other tags"]);
        assert_eq!(out.len(), 12 + 18 + image.len());
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize, out.len() - 8);
        assert_eq!(out[20], 0, "the EXIF and XMP flags are off");
    }

    /// A minimal HEIC: ftyp, then meta with an EXIF item stored in idat.
    #[test]
    fn a_heic_has_its_exif_blanked_in_place() {
        let bx = |kind: &[u8; 4], body: &[u8]| [&((body.len() + 8) as u32).to_be_bytes()[..], kind, body].concat();
        let mut payload = 6u32.to_be_bytes().to_vec();
        payload.extend_from_slice(b"Exif\0\0");
        payload.extend_from_slice(&exif_with_gps(1));
        let infe = bx(b"infe", &[&[2, 0, 0, 0][..], &1u16.to_be_bytes(), &[0, 0], b"Exif", b"\0"].concat());
        let iinf = bx(b"iinf", &[&[0, 0, 0, 0][..], &1u16.to_be_bytes(), &infe].concat());
        // Version 1: offsets and lengths of 4 bytes, no base; the item in idat (method 1).
        let iloc_body = [&[1, 0, 0, 0][..], &[0x44, 0x00], &1u16.to_be_bytes(), &1u16.to_be_bytes(), &1u16.to_be_bytes(), &0u16.to_be_bytes(), &1u16.to_be_bytes(), &0u32.to_be_bytes(), &(payload.len() as u32).to_be_bytes()].concat();
        let iloc = bx(b"iloc", &iloc_body);
        let idat = bx(b"idat", &payload);
        let meta = bx(b"meta", &[&[0, 0, 0, 0][..], &iinf, &iloc, &idat].concat());
        let input = [bx(b"ftyp", b"heic\0\0\0\0mif1heic"), meta].concat();
        let (out, found) = scrub_bytes(input.clone()).unwrap();
        assert_eq!(words(&found), ["location", "camera", "dates"]);
        assert_eq!(out.len(), input.len());
        assert!(find(&out, b"Apple").is_none() && find(&out, b"2024").is_none());
        assert!(find(&out, b"Exif\0\0MM\0*\0\0\0\x08\0\0").is_some(), "a valid, empty EXIF is left");
        assert!(scrub_bytes(out).unwrap().1.is_empty());
    }

    #[test]
    fn a_pdf_loses_its_document_info_and_xmp_and_every_offset_holds() {
        let xmp = "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:Description xmp:CreateDate=\"2024-06-01\" dc:creator=\"Ann\"/></x:xmpmeta><?xpacket end=\"w\"?>";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_string(),
            "<< /Type /Pages /Kids [] /Count 0 >>".to_string(),
            "<< /Author (Ann \\(Smith\\)) /Creator (Microsoft Word) /Producer (macOS Quartz) /CreationDate (D:20240601120000Z) /Title <48656c6c6f> >>".to_string(),
            format!("<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{xmp}\nendstream", xmp.len()),
        ];
        let mut pdf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n% created by Secret Writer 1.0\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for o in &offsets {
            pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(format!("trailer\n<< /Size 5 /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());

        let (out, found) = scrub_bytes(pdf.clone()).unwrap();
        assert_eq!(words(&found), ["author", "dates", "software", "title"]);
        assert_eq!(out.len(), pdf.len());
        for gone in ["Ann", "Microsoft Word", "Quartz", "D:2024", "2024-06-01", "Secret Writer"] {
            assert!(find(&out, gone.as_bytes()).is_none(), "{gone}");
        }
        // Every object is where the cross-reference table says.
        for (i, o) in offsets.iter().enumerate() {
            assert!(out[*o..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
        assert!(find(&out, b"3 0 obj\n<<").is_some());
        assert!(out.starts_with(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n%  "), "the version and the binary marker stay");
        assert!(find(&out, EMPTY_XMP).is_some());
        assert!(scrub_bytes(out).unwrap().1.is_empty());
    }

    /// A Word file with an author, a company, a comment by a named reviewer and a photo with GPS.
    fn docx(path: &Path) {
        let mut w = ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let mut photo = jpeg_segment(0xE1, &[&b"Exif\0\0"[..], &exif_with_gps(1)].concat());
        photo = jpeg(&[photo], b"");
        let parts: [(&str, &[u8]); 6] = [
            ("[Content_Types].xml", b"<Types/>"),
            ("docProps/core.xml", b"<cp:coreProperties><dc:creator>Ann Smith</dc:creator><cp:lastModifiedBy>Bob</cp:lastModifiedBy><dcterms:created xsi:type=\"dcterms:W3CDTF\">2024-06-01T12:00:00Z</dcterms:created></cp:coreProperties>"),
            ("docProps/app.xml", b"<Properties><Application>Microsoft Office Word</Application><Company>Acme Corp</Company><Pages>1</Pages></Properties>"),
            ("word/document.xml", b"<w:document><w:body><w:ins w:id=\"1\" w:author=\"Bob\" w:date=\"2024-06-02T00:00:00Z\"><w:r><w:t>Hello</w:t></w:r></w:ins></w:body></w:document>"),
            ("word/comments.xml", b"<w:comments><w:comment w:id=\"0\" w:author=\"Carol\" w:initials=\"C\"><w:p><w:r><w:t>Looks good</w:t></w:r></w:p></w:comment></w:comments>"),
            ("word/media/image1.jpeg", &photo),
        ];
        for (name, data) in parts {
            w.start_file(name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    fn part(zip: &mut ZipArchive<File>, name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn a_word_file_loses_its_author_company_and_reviewers_but_keeps_its_text() {
        let dir = std::env::temp_dir().join(format!("sshee-scrub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("report.docx");
        docx(&path);
        let before = std::fs::read(&path).unwrap();

        let (scrub, copy) = clean_copy(&path).unwrap();
        let Scrub::Removed(found) = scrub else { panic!("{scrub:?}") };
        assert_eq!(words(&found), ["location", "camera", "author", "company", "reviewer names", "dates", "software"]);
        assert_eq!(std::fs::read(&path).unwrap(), before, "the file you picked isn't touched");

        let copy = copy.unwrap();
        assert_eq!(copy.len(), std::fs::metadata(copy.path()).unwrap().len());
        let mut zip = ZipArchive::new(File::open(copy.path()).unwrap()).unwrap();
        assert_eq!(part(&mut zip, "docProps/core.xml"), EMPTY_CORE.as_bytes());
        assert_eq!(part(&mut zip, "docProps/app.xml"), EMPTY_APP.as_bytes());
        let document = String::from_utf8(part(&mut zip, "word/document.xml")).unwrap();
        assert_eq!(document, "<w:document><w:body><w:ins w:id=\"1\" w:author=\"\"><w:r><w:t>Hello</w:t></w:r></w:ins></w:body></w:document>");
        let comments = String::from_utf8(part(&mut zip, "word/comments.xml")).unwrap();
        assert!(comments.contains("w:author=\"\" w:initials=\"\"") && comments.contains("Looks good"));
        assert!(find(&part(&mut zip, "word/media/image1.jpeg"), b"Apple").is_none());
        assert_eq!(part(&mut zip, "[Content_Types].xml"), b"<Types/>");

        let kept = copy.path().to_path_buf();
        drop(copy);
        assert!(!kept.exists(), "the copy goes once it's sent");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn other_files_go_as_they_are() {
        let dir = std::env::temp_dir().join(format!("sshee-scrub-other-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = dir.join("notes.txt");
        std::fs::write(&text, "Hello").unwrap();
        assert!(matches!(clean_copy(&text).unwrap(), (Scrub::AsIs, None)));
        // A zip that isn't an Office file.
        let zip = dir.join("site.zip");
        let mut w = ZipWriter::new(File::create(&zip).unwrap());
        w.start_file("index.html", SimpleFileOptions::default()).unwrap();
        w.write_all(b"hi").unwrap();
        w.finish().unwrap();
        assert!(matches!(clean_copy(&zip).unwrap(), (Scrub::AsIs, None)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn counts_what_came_out_of_a_batch() {
        let mut c = Cleaning::default();
        c.add(&Scrub::Removed(Found::from([Trace::Author, Trace::Dates])));
        c.add(&Scrub::Removed(Found::from([Trace::Location])));
        c.add(&Scrub::Nothing);
        c.add(&Scrub::AsIs);
        assert_eq!((c.cleaned, c.clean, c.as_is), (2, 1, 1));
        assert_eq!(c.removed, ["location", "author", "dates"]);
        assert_eq!(serde_json::to_value(&c).unwrap()["asIs"], 1);
    }
}
