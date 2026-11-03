//! Photos and videos for the preview in Files. A video player reads from any
//! place, a little at a time: a server's file comes over a connection kept
//! open for it, so the next piece doesn't sign in again.
//!
//! A page gets them from a small web server on this computer, which only
//! knows the files open right now, by ids nothing else can guess. It's what
//! players work best with, and on Linux the only way a video plays: WebKit
//! there can't read one from an app's own scheme. `MediaFile::answer` is for
//! that scheme, where a plain local address isn't allowed.

use crate::error::OpError;
use crate::files::resolve_local;
use crate::ssh::{run_remote_read, shell_quote, spawn_error, transfer_command, SshEnv, SshTarget, Target};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What's read from a server at a time.
const PIECE: usize = 256 * 1024;
/// Pieces kept, for a player that asks for a place it has just read.
const KEPT: usize = 64;
/// Reading on to a place this near beats signing in again.
const READ_ON: u64 = 4 * 1024 * 1024;
/// Players go back and forth between a video's start and its end, where its
/// index may be: each gets a connection, and neither is dropped for the other.
const CONNECTIONS: usize = 2;
/// The most one `answer` holds, as all of it is in memory until it's sent.
const MAX_ANSWER: u64 = 1024 * 1024;
/// What `answer` holds of a file asked for whole: a picture.
const MAX_WHOLE: u64 = 128 * 1024 * 1024;

/// The top-level parts looked through in a video with Tor on, and the most of its
/// index read. A file that needs more isn't played.
const MAX_PARTS: usize = 256;
const MAX_INDEX: u64 = 64 * 1024 * 1024;

/// A file open for the preview.
pub struct MediaFile {
    size: u64,
    mime: &'static str,
    source: Source,
    closed: AtomicBool,
    /// The connections, apart from the rest: one can be ended while a read waits on it.
    running: Mutex<HashMap<u64, Child>>,
    /// The answers going out, which closing the file ends however long they've waited on a player.
    answering: Mutex<HashMap<u64, TcpStream>>,
    next_answer: AtomicU64,
}

enum Source {
    Local(Mutex<File>),
    Server { t: Box<SshTarget>, env: Box<SshEnv>, path: String, reads: Mutex<Reads> },
}

#[derive(Default)]
struct Reads {
    /// The one used longest ago first.
    readers: Vec<Reader>,
    pieces: VecDeque<(u64, Vec<u8>)>,
    next_id: u64,
}

struct Reader {
    id: u64,
    stream: ChildStdout,
    /// The place in the file its next byte is from.
    at: u64,
}

/// What to send back for a request, held in memory.
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub content_range: Option<String>,
    pub body: Vec<u8>,
}

/// Which part of a file a request gets.
struct Plan {
    status: u16,
    first: u64,
    length: u64,
    content_range: Option<String>,
}

impl Reads {
    fn kept(&self, place: u64) -> Option<&[u8]> {
        let (start, piece) = self.pieces.iter().rev().find(|(start, piece)| *start <= place && place < start + piece.len() as u64)?;
        Some(&piece[(place - start) as usize..])
    }
}

impl MediaFile {
    pub fn open(target: &Target, env: &SshEnv, path: &str) -> Result<Self, OpError> {
        let (size, source) = match target {
            Target::Ssh(t) => {
                // stat, as `wc` may read all of a big video to count it.
                let script = format!(
                    r#"F={}; [ -f "$F" ] || {{ echo "This isn't a regular file." >&2; exit 4; }}; stat -L -c %s -- "$F" 2>/dev/null || stat -L -f %z -- "$F" 2>/dev/null || wc -c < "$F""#,
                    shell_quote(path)
                );
                let out = run_remote_read(t, env, &script)?;
                // The last line: a login script may have said something first.
                let size = String::from_utf8_lossy(&out).lines().last().and_then(|l| l.trim().parse().ok());
                let size = size.ok_or_else(|| OpError::failed("Couldn't tell how big this file is."))?;
                (size, Source::Server { t: Box::new(t.clone()), env: Box::new(env.clone()), path: path.into(), reads: Mutex::default() })
            }
            Target::Local => {
                let p = resolve_local(path);
                let meta = std::fs::metadata(&p)?;
                // A device or a pipe (/dev/zero, a FIFO) never ends.
                if !meta.is_file() {
                    return Err(OpError::failed("This isn't a regular file."));
                }
                (meta.len(), Source::Local(Mutex::new(File::open(&p)?)))
            }
        };
        let file = Self { size, mime: mime_type(path), source, closed: AtomicBool::new(false), running: Mutex::default(), answering: Mutex::default(), next_answer: AtomicU64::new(0) };
        if file.mime.starts_with("video/") && crate::tor::is_on() && !file.stays_local()? {
            return Err(OpError::failed("With Tor on, this video isn't played here: the way it's made could send this computer's player to the internet, around Tor. Download it to watch it."));
        }
        Ok(file)
    }

    /// Whether a video keeps the player to this file. The system's player has
    /// its own way to the internet, around Tor, and some kinds of file tell it
    /// to go there: a playlist, whatever its name says, or a QuickTime movie
    /// with its picture elsewhere. WebM, Matroska and Ogg have no such thing.
    /// MP4 and QuickTime files say where their data is in their index.
    fn stays_local(&self) -> Result<bool, OpError> {
        let start = self.read(0, 4)?;
        if start == [0x1a, 0x45, 0xdf, 0xa3] || start == *b"OggS" {
            return Ok(true);
        }
        let (mut at, mut indexed) = (0, false);
        for _ in 0..MAX_PARTS {
            if at >= self.size {
                return Ok(indexed);
            }
            let head = self.read(at, 16)?;
            let Some((kind, body, length)) = part_at(&head, self.size - at) else {
                return Ok(false);
            };
            if &kind == b"moov" {
                if length > MAX_INDEX || !index_stays_local(&self.read(at + body, length - body)?) {
                    return Ok(false);
                }
                indexed = true;
            }
            at += length;
        }
        Ok(false)
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn mime(&self) -> &'static str {
        self.mime
    }

    fn plan(&self, range: Option<&str>) -> Plan {
        let Some(range) = range else {
            return Plan { status: 200, first: 0, length: self.size, content_range: None };
        };
        match asked_range(range, self.size) {
            Some((first, last)) => Plan { status: 206, first, length: last - first + 1, content_range: Some(format!("bytes {first}-{last}/{}", self.size)) },
            None => Plan { status: 416, first: 0, length: 0, content_range: Some(format!("bytes */{}", self.size)) },
        }
    }

    /// The answer to a request, with its `Range` header if it has one, for
    /// what can only hand over one in full. A long range gets its start, and
    /// the player asks again for the rest.
    pub fn answer(&self, range: Option<&str>) -> Result<Answer, OpError> {
        let plan = self.plan(range);
        match plan.status {
            206 => {
                let body = self.read(plan.first, plan.length.min(MAX_ANSWER))?;
                if body.is_empty() {
                    return Ok(Answer { status: 416, content_range: Some(format!("bytes */{}", self.size)), body });
                }
                let content_range = format!("bytes {}-{}/{}", plan.first, plan.first + body.len() as u64 - 1, self.size);
                Ok(Answer { status: 206, content_range: Some(content_range), body })
            }
            200 if plan.length > MAX_WHOLE => Ok(Answer { status: 413, content_range: None, body: Vec::new() }),
            200 => Ok(Answer { status: 200, content_range: None, body: self.read(0, plan.length)? }),
            status => Ok(Answer { status, content_range: plan.content_range, body: Vec::new() }),
        }
    }

    /// `want` bytes from `at`, or fewer if the file ends first.
    fn read(&self, at: u64, want: u64) -> Result<Vec<u8>, OpError> {
        let want = want.min(self.size.saturating_sub(at)) as usize;
        match &self.source {
            Source::Local(file) => {
                let mut file = file.lock().unwrap();
                let mut out = Vec::with_capacity(want);
                file.seek(SeekFrom::Start(at))?;
                (&mut *file).take(want as u64).read_to_end(&mut out)?;
                Ok(out)
            }
            Source::Server { t, env, path, reads } => {
                let mut reads = reads.lock().unwrap();
                self.read_server(&mut reads, t, env, path, at, want)
            }
        }
    }

    fn read_server(&self, reads: &mut Reads, t: &SshTarget, env: &SshEnv, path: &str, at: u64, want: usize) -> Result<Vec<u8>, OpError> {
        let mut out = Vec::with_capacity(want);
        let mut tries = 0;
        while out.len() < want {
            let place = at + out.len() as u64;
            if let Some(kept) = reads.kept(place) {
                let n = kept.len().min(want - out.len());
                out.extend_from_slice(&kept[..n]);
                continue;
            }
            if self.closed.load(Ordering::SeqCst) {
                return Err(OpError::failed("This file was closed."));
            }
            let mut reader = match reads.readers.iter().position(|r| r.at <= place && place - r.at <= READ_ON) {
                Some(i) => reads.readers.remove(i),
                None => {
                    if reads.readers.len() >= CONNECTIONS {
                        let oldest = reads.readers.remove(0);
                        self.end(oldest.id);
                    }
                    reads.next_id += 1;
                    self.connect(t, env, path, place, reads.next_id)?
                }
            };
            let mut piece = Vec::with_capacity(PIECE);
            // An error here is how a connection ends, as is the end of the file.
            let _ = (&mut reader.stream).take(PIECE as u64).read_to_end(&mut piece);
            if piece.is_empty() {
                self.end(reader.id);
                tries += 1;
                // Once more: it may have been moved to a new route. Then the
                // connection's gone, or the file has become shorter.
                if tries == 2 {
                    return if out.is_empty() { Err(OpError::failed("Lost the connection while reading this file.")) } else { Ok(out) };
                }
                continue;
            }
            tries = 0;
            let start = reader.at;
            reader.at += piece.len() as u64;
            reads.pieces.push_back((start, piece));
            if reads.pieces.len() > KEPT {
                reads.pieces.pop_front();
            }
            reads.readers.push(reader);
        }
        Ok(out)
    }

    /// Keeps sending the file from `at` on, as fast as it's read: ssh stops
    /// taking more from the server while nobody reads, as when a video's paused.
    fn connect(&self, t: &SshTarget, env: &SshEnv, path: &str, at: u64, id: u64) -> Result<Reader, OpError> {
        let mut cmd = transfer_command(t, env, &format!("tail -c +{} -- {}", at + 1, shell_quote(path)));
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut ssh = cmd.spawn().map_err(spawn_error)?;
        let stream = ssh.stdout.take().expect("piped stdout");
        self.running.lock().unwrap().insert(id, ssh);
        // Closed meanwhile: nothing else would end this one.
        if self.closed.load(Ordering::SeqCst) {
            self.end(id);
            return Err(OpError::failed("This file was closed."));
        }
        Ok(Reader { id, stream, at })
    }

    fn end(&self, id: u64) {
        if let Some(mut ssh) = self.running.lock().unwrap().remove(&id) {
            let _ = ssh.kill();
            let _ = ssh.wait();
        }
    }

    /// Ends every connection; the next read opens its own, the way things go now.
    fn end_connections(&self) {
        for (_, mut ssh) in self.running.lock().unwrap().drain() {
            let _ = ssh.kill();
            let _ = ssh.wait();
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.end_connections();
        for (_, stream) in self.answering.lock().unwrap().drain() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    /// Sends a request its answer, which closing the file can end.
    fn send(&self, stream: &mut TcpStream, plan: &Plan, body: bool) -> io::Result<()> {
        let id = self.next_answer.fetch_add(1, Ordering::SeqCst);
        self.answering.lock().unwrap().insert(id, stream.try_clone()?);
        let sent = if self.closed.load(Ordering::SeqCst) {
            Err(io::ErrorKind::NotConnected.into())
        } else {
            self.send_now(stream, plan, body)
        };
        self.answering.lock().unwrap().remove(&id);
        sent
    }

    fn send_now(&self, stream: &mut TcpStream, plan: &Plan, body: bool) -> io::Result<()> {
        stream.write_all(head(plan.status, self.mime, plan.length, plan.content_range.as_deref()).as_bytes())?;
        let mut sent = 0;
        while body && sent < plan.length {
            let piece = self.read(plan.first + sent, (plan.length - sent).min(PIECE as u64)).map_err(|e| io::Error::other(e.message))?;
            if piece.is_empty() {
                break;
            }
            stream.write_all(&piece)?;
            sent += piece.len() as u64;
        }
        Ok(())
    }
}

impl Drop for MediaFile {
    fn drop(&mut self) {
        self.close();
    }
}

/// The files the preview has open.
#[derive(Default)]
pub struct MediaFiles {
    /// By id, with the window that opened each: a window that closes takes no one's leave.
    open: Mutex<HashMap<String, (String, Arc<MediaFile>)>>,
    /// Where they're served from, once one has been opened.
    port: Mutex<Option<u16>>,
}

/// A file ready to be read: `url` is all a page needs.
#[derive(Debug, Serialize)]
pub struct OpenedMedia {
    pub id: String,
    pub size: u64,
    pub url: String,
}

impl MediaFiles {
    pub fn add(self: &Arc<Self>, file: MediaFile, window: &str) -> Result<OpenedMedia, OpError> {
        let port = self.serving().map_err(|e| OpError::failed(format!("Couldn't get the preview ready: {e}")))?;
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| OpError::failed(e.to_string()))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let size = file.size();
        self.open.lock().unwrap().insert(id.clone(), (window.to_string(), Arc::new(file)));
        Ok(OpenedMedia { url: format!("http://{}:{port}/{id}", Ipv4Addr::LOCALHOST), id, size })
    }

    fn serving(self: &Arc<Self>) -> io::Result<u16> {
        let mut port = self.port.lock().unwrap();
        if let Some(port) = *port {
            return Ok(port);
        }
        // This computer only.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let bound = listener.local_addr()?.port();
        let files = self.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    // Out of file handles, say: no reason to spin until there are some.
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                };
                let files = files.clone();
                let _ = std::thread::Builder::new().spawn(move || {
                    let _ = serve(stream, &files);
                });
            }
        });
        *port = Some(bound);
        Ok(bound)
    }

    pub fn get(&self, id: &str) -> Option<Arc<MediaFile>> {
        self.open.lock().unwrap().get(id).map(|(_, file)| file.clone())
    }

    pub fn close(&self, id: &str) {
        let file = self.open.lock().unwrap().remove(id);
        if let Some((_, file)) = file {
            file.close();
        }
    }

    /// Closes what a window that's gone had open.
    pub fn close_for_window(&self, window: &str) {
        let ids: Vec<_> = self.open.lock().unwrap().iter().filter(|(_, (w, _))| w == window).map(|(id, _)| id.clone()).collect();
        for id in ids {
            self.close(&id);
        }
    }

    /// Tor went on or off, or got new routes: what's being read moves too.
    pub fn reroute(&self) {
        let files: Vec<_> = self.open.lock().unwrap().values().map(|(_, file)| file.clone()).collect();
        for file in files {
            file.end_connections();
        }
    }

    pub fn close_all(&self) {
        let files: Vec<_> = self.open.lock().unwrap().drain().map(|(_, (_, file))| file).collect();
        for file in files {
            file.close();
        }
    }
}

/// One request, one answer, and the connection closes. A long answer goes out
/// as it's read, and waits while the player has enough: then ssh waits too.
fn serve(mut stream: TcpStream, files: &MediaFiles) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = read_request(&mut stream)?;
    let mut lines = request.split("\r\n");
    let mut first_line = lines.next().unwrap_or_default().split(' ');
    let (method, address) = (first_line.next().unwrap_or_default(), first_line.next().unwrap_or_default());
    let Some(file) = address.strip_prefix('/').and_then(|id| files.get(id)) else {
        return stream.write_all(head(404, "application/octet-stream", 0, None).as_bytes());
    };
    if method != "GET" && method != "HEAD" {
        return stream.write_all(head(405, "application/octet-stream", 0, None).as_bytes());
    }
    let range = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("range").then(|| value.trim())
    });
    let plan = file.plan(range);
    file.send(&mut stream, &plan, method != "HEAD")
}

/// A request's first line and headers.
fn read_request(stream: &mut TcpStream) -> io::Result<String> {
    let mut request = Vec::new();
    let mut byte = [0u8; 1024];
    while !request.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut byte)?;
        // No file's request is this long, or ends early.
        if n == 0 || request.len() + n > 16 * 1024 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        request.extend_from_slice(&byte[..n]);
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

/// Never stored, and never anything a page could run, should one be pointed at it.
fn head(status: u16, mime: &str, length: u64, content_range: Option<&str>) -> String {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Range Not Satisfiable",
    };
    let range = content_range.map(|r| format!("Content-Range: {r}\r\n")).unwrap_or_default();
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {length}\r\n{range}Accept-Ranges: bytes\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; sandbox\r\nConnection: close\r\n\r\n"
    )
}

/// A part of an MP4 or QuickTime file from the start of its header, with
/// `left` bytes from there to the file's end: its kind, how long its header is,
/// and its length with the header. None for anything else.
fn part_at(head: &[u8], left: u64) -> Option<([u8; 4], u64, u64)> {
    let kind: [u8; 4] = head.get(4..8)?.try_into().ok()?;
    if !kind.iter().all(|b| b.is_ascii_alphanumeric() || *b == b' ') {
        return None;
    }
    let short = u32::from_be_bytes(head.get(..4)?.try_into().ok()?);
    let (body, length) = match short {
        0 => (8, left),
        1 => (16, u64::from_be_bytes(head.get(8..16)?.try_into().ok()?)),
        n => (8, u64::from(n)),
    };
    (length >= body && length <= left).then_some((kind, body, length))
}

/// Whether an MP4 or QuickTime index keeps to its own file. A reference movie
/// is one that points elsewhere, and a compressed index can hide anything. Each
/// place data is said to be must carry the flag "in this file". Anything in the
/// index that merely looks like these counts too: the worst of that is a
/// video to download.
fn index_stays_local(index: &[u8]) -> bool {
    let u32_at = |at: usize| index.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().expect("four bytes")));
    let found = |kind: &[u8; 4]| index.windows(4).any(|w| w == kind);
    if found(b"rmra") || found(b"rmda") || found(b"cmov") {
        return false;
    }
    index.windows(4).enumerate().filter(|(_, w)| w == b"dref").all(|(at, _)| {
        // After the kind: version and flags, how many, then the places.
        let Some(places) = u32_at(at + 8) else { return false };
        let mut place = at + 12;
        (0..places).all(|_| {
            let (Some(size), Some(flags)) = (u32_at(place), u32_at(place + 8)) else { return false };
            place += size as usize;
            size >= 12 && flags & 1 == 1
        })
    })
}

/// What a `Range` header asks for, as its first and last byte in a file of
/// `size`. None when that can't be met. Of a list of ranges, the first.
fn asked_range(header: &str, size: u64) -> Option<(u64, u64)> {
    let (first, last) = header.strip_prefix("bytes=")?.split(',').next()?.trim().split_once('-')?;
    let end = size.checked_sub(1)?;
    if first.is_empty() {
        // The last n bytes.
        let n: u64 = last.parse().ok()?;
        return (n > 0).then(|| (size.saturating_sub(n), end));
    }
    let first: u64 = first.parse().ok()?;
    let last = if last.is_empty() { end } else { last.parse::<u64>().ok()?.min(end) };
    (first <= last).then_some((first, last))
}

/// The type a file is sent as, by its name. Nothing a page would run or lay
/// out: what isn't known goes as plain bytes, and `<img>` draws an SVG without
/// running what's in it.
fn mime_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sshee-media-{}-{name}", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn a_range_is_read_as_a_player_means_it() {
        assert_eq!(asked_range("bytes=0-1", 10), Some((0, 1)));
        assert_eq!(asked_range("bytes=4-", 10), Some((4, 9)));
        assert_eq!(asked_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(asked_range("bytes=-30", 10), Some((0, 9)));
        assert_eq!(asked_range("bytes=8-100", 10), Some((8, 9)));
        assert_eq!(asked_range("bytes=2-3, 6-7", 10), Some((2, 3)));
        // Can't be met.
        assert_eq!(asked_range("bytes=10-", 10), None);
        assert_eq!(asked_range("bytes=5-2", 10), None);
        assert_eq!(asked_range("bytes=-0", 10), None);
        assert_eq!(asked_range("bytes=0-", 0), None);
        assert_eq!(asked_range("seconds=1-2", 10), None);
        assert_eq!(asked_range("bytes=a-b", 10), None);
    }

    #[test]
    fn answers_say_which_bytes_they_hold() {
        let path = temp("answers.png", b"0123456789");
        let file = MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap();
        assert_eq!((file.size(), file.mime()), (10, "image/png"));
        let whole = file.answer(None).unwrap();
        assert_eq!((whole.status, whole.content_range, whole.body.as_slice()), (200, None, &b"0123456789"[..]));
        let first_two = file.answer(Some("bytes=0-1")).unwrap();
        assert_eq!((first_two.status, first_two.content_range.as_deref(), first_two.body.as_slice()), (206, Some("bytes 0-1/10"), &b"01"[..]));
        let rest = file.answer(Some("bytes=7-")).unwrap();
        assert_eq!((rest.status, rest.content_range.as_deref(), rest.body.as_slice()), (206, Some("bytes 7-9/10"), &b"789"[..]));
        let past = file.answer(Some("bytes=10-")).unwrap();
        assert_eq!((past.status, past.content_range.as_deref(), past.body.len()), (416, Some("bytes */10"), 0));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_long_video_comes_a_piece_at_a_time() {
        let path = temp("long.mp4", &vec![7u8; (MAX_ANSWER + 10) as usize]);
        let file = MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap();
        let start = file.answer(Some("bytes=0-")).unwrap();
        assert_eq!((start.status, start.body.len() as u64), (206, MAX_ANSWER));
        assert_eq!(start.content_range.unwrap(), format!("bytes 0-{}/{}", MAX_ANSWER - 1, MAX_ANSWER + 10));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn only_a_file_opens() {
        let dir = std::env::temp_dir();
        assert!(MediaFile::open(&Target::Local, &SshEnv::default(), &dir.to_string_lossy()).is_err());
    }

    #[test]
    fn a_page_gets_nothing_it_would_run() {
        for name in ["a.html", "a.htm", "a.js", "a.xhtml", "a.xml", "noext", "a.m3u8"] {
            assert_eq!(mime_type(name), "application/octet-stream", "{name}");
        }
        assert_eq!(mime_type("/a/b/PHOTO.JPG"), "image/jpeg");
        assert_eq!(mime_type("clip.MOV"), "video/quicktime");
    }

    #[test]
    fn a_closed_file_cant_be_found_again() {
        let path = temp("closed.gif", b"x");
        let files = Arc::new(MediaFiles::default());
        let opened = files.add(MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap(), "main").unwrap();
        assert_eq!(opened.id.len(), 32);
        assert!(files.get(&opened.id).is_some());
        assert!(files.get("nothing").is_none());
        files.close(&opened.id);
        assert!(files.get(&opened.id).is_none());
        std::fs::remove_file(path).unwrap();
    }

    fn ask(url: &str, request: &str) -> (String, Vec<u8>) {
        let (host, path) = url.strip_prefix("http://").unwrap().split_once('/').unwrap();
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(request.replace("{path}", path).as_bytes()).unwrap();
        let mut all = Vec::new();
        stream.read_to_end(&mut all).unwrap();
        let end = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&all[..end]).into_owned(), all[end + 4..].to_vec())
    }

    #[test]
    fn its_web_server_gives_what_a_player_asks_for() {
        let content: Vec<u8> = (0..(PIECE * 2 + 5)).map(|i| i as u8).collect();
        let path = temp("served.webm", &content);
        let files = Arc::new(MediaFiles::default());
        let opened = files.add(MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap(), "main").unwrap();
        assert!(opened.url.starts_with("http://127.0.0.1:"), "{}", opened.url);

        let (head, body) = ask(&opened.url, "GET /{path} HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        assert!(head.contains("Content-Type: video/webm"), "{head}");
        assert!(head.contains("Cache-Control: no-store") && head.contains("nosniff"), "{head}");
        assert_eq!(body, content);

        // A range is sent in full however long, across the pieces it's read in.
        let (head, body) = ask(&opened.url, "GET /{path} HTTP/1.1\r\nrange: bytes=5-\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 206 Partial Content"), "{head}");
        assert!(head.contains(&format!("Content-Range: bytes 5-{}/{}", content.len() - 1, content.len())), "{head}");
        assert_eq!(body, &content[5..]);

        let (head, body) = ask(&opened.url, "GET /{path} HTTP/1.1\r\nRange: bytes=0-1\r\n\r\n");
        assert!(head.contains(&format!("Content-Range: bytes 0-1/{}", content.len())), "{head}");
        assert_eq!(body, &content[..2]);

        let (head, body) = ask(&opened.url, "HEAD /{path} HTTP/1.1\r\n\r\n");
        assert!(head.contains(&format!("Content-Length: {}", content.len())), "{head}");
        assert!(body.is_empty());

        let (head, _) = ask(&opened.url, "GET /{path} HTTP/1.1\r\nRange: bytes=99999999-\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 416"), "{head}");
        assert!(head.contains(&format!("Content-Range: bytes */{}", content.len())), "{head}");

        let (head, _) = ask(&opened.url, "POST /{path} HTTP/1.1\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 405"), "{head}");

        // Without the id, or once closed, there's nothing to get.
        let (head, _) = ask(&opened.url, "GET / HTTP/1.1\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        files.close(&opened.id);
        let (head, _) = ask(&opened.url, "GET /{path} HTTP/1.1\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_window_that_closes_closes_its_files() {
        let path = temp("window.gif", b"x");
        let open = || MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap();
        let files = Arc::new(MediaFiles::default());
        let (mine, other) = (files.add(open(), "server-1").unwrap(), files.add(open(), "main").unwrap());
        files.close_for_window("server-1");
        assert!(files.get(&mine.id).is_none());
        assert!(files.get(&other.id).is_some());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn closing_a_file_ends_an_answer_waiting_on_a_player() {
        // Far more than the system's socket buffers hold, with nobody reading.
        let path = temp("waiting.webm", &vec![0u8; 64 * 1024 * 1024]);
        let files = Arc::new(MediaFiles::default());
        let opened = files.add(MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap(), "main").unwrap();
        let file = files.get(&opened.id).unwrap();
        let (host, id) = opened.url.strip_prefix("http://").unwrap().split_once('/').unwrap();
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(format!("GET /{id} HTTP/1.1\r\n\r\n").as_bytes()).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while file.answering.lock().unwrap().is_empty() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(file.answering.lock().unwrap().len(), 1);
        files.close(&opened.id);
        // Its thread lets go of the file: this is the last hold on it.
        while Arc::strong_count(&file) > 1 && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(Arc::strong_count(&file), 1);
        std::fs::remove_file(path).unwrap();
    }

    fn part(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// An index with one place data can be, with its flags.
    fn index_with_place(flags: u32, url: &[u8]) -> Vec<u8> {
        let mut place = flags.to_be_bytes().to_vec();
        place.extend_from_slice(url);
        let mut dref = vec![0, 0, 0, 0, 0, 0, 0, 1];
        dref.extend(part(b"url ", &place));
        part(b"moov", &part(b"trak", &part(b"dref", &dref)))
    }

    fn stays_local(name: &str, content: &[u8]) -> bool {
        let path = temp(name, content);
        let file = MediaFile::open(&Target::Local, &SshEnv::default(), &path.to_string_lossy()).unwrap();
        let answer = file.stays_local().unwrap();
        std::fs::remove_file(path).unwrap();
        answer
    }

    #[test]
    fn with_tor_on_a_video_plays_only_if_it_keeps_to_itself() {
        let ftyp = part(b"ftyp", b"isom");
        let mdat = part(b"mdat", &[9; 64]);
        let own = index_with_place(1, b"");
        let elsewhere = index_with_place(0, b"http://198.51.100.7/tracker.mov\0");

        // Formats without such a thing.
        assert!(stays_local("a.webm", &[0x1a, 0x45, 0xdf, 0xa3, 1, 2, 3]));
        assert!(stays_local("a.ogv", b"OggS\0\0\0"));
        // MP4 and QuickTime with their data inside, the index before it or after.
        assert!(stays_local("a.mp4", &[ftyp.clone(), own.clone(), mdat.clone()].concat()));
        assert!(stays_local("a.mov", &[mdat.clone(), own.clone()].concat()));

        // A reference to somewhere else.
        assert!(!stays_local("b.mov", &[ftyp.clone(), elsewhere.clone(), mdat.clone()].concat()));
        assert!(!stays_local("c.mov", &[mdat.clone(), own.clone(), elsewhere].concat()), "a second index counts too");
        assert!(!stays_local("d.mov", &[ftyp.clone(), part(b"moov", &part(b"rmra", b"")), mdat.clone()].concat()));
        // An index that can't be read.
        assert!(!stays_local("e.mov", &[ftyp.clone(), part(b"moov", &part(b"cmov", b"")), mdat.clone()].concat()));
        // No index at all.
        assert!(!stays_local("f.mp4", &[ftyp, mdat].concat()));
        // A playlist or anything else, whatever its name says.
        assert!(!stays_local("g.mp4", b"#EXTM3U\n#EXT-X-VERSION:3\nhttp://198.51.100.7/a.ts\n"));
        assert!(!stays_local("h.mkv", b"not a video"));
        assert!(!stays_local("i.mp4", b""));
        // A length past the file's end, or too short to be one.
        assert!(!stays_local("j.mp4", &[0, 0, 0xff, 0xff, b'm', b'o', b'o', b'v', 0, 0]));
        assert!(!stays_local("k.mp4", &[0, 0, 0, 2, b'm', b'd', b'a', b't']));
    }

    #[test]
    fn a_data_reference_that_runs_off_the_index_is_no_good() {
        let mut truncated = index_with_place(1, b"");
        truncated.truncate(truncated.len() - 6);
        assert!(!index_stays_local(&truncated[8..]));
        // More places than there are.
        let mut dref = vec![0, 0, 0, 0, 0, 0, 0, 9];
        dref.extend(part(b"url ", &1u32.to_be_bytes()));
        assert!(!index_stays_local(&part(b"dref", &dref)));
    }

    /// `kept` finds a place in what's been read, the newest copy first.
    #[test]
    fn what_was_just_read_is_found_again() {
        let mut reads = Reads::default();
        reads.pieces.push_back((10, vec![1, 2, 3]));
        reads.pieces.push_back((11, vec![9, 8]));
        assert_eq!(reads.kept(10), Some(&[1u8, 2, 3][..]));
        assert_eq!(reads.kept(12), Some(&[8u8][..]));
        assert_eq!(reads.kept(13), None);
        assert_eq!(reads.kept(9), None);
    }
}
