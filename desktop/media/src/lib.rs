//! SSHeesh's video player, for the preview in Files: MP4 (H.264 with AAC) and WebM (VP9 with
//! Opus or Vorbis; H.264 in Matroska too), with sound, on Linux, macOS and Windows alike.
//! Everything it decodes with is built into the program, so nothing has to be installed.
//!
//! A [`Player`] opens a file on a thread of its own and does all its work there: reading,
//! decoding, keeping the picture to the sound. The screen only tells it what to do (play,
//! pause, seek, volume) and reads [`View`]; pictures come to a [`Screen`] as they're due.
//! Dropping the player stops it: the thread ends, the sound with it.
//!
//! The file is untrusted. Its sizes are checked before they're believed ([`demux`]), a
//! picture bigger than [`video::MAX_PIXELS`] is refused before any is decoded, and whatever
//! goes wrong while decoding, a panic included, ends as a [`Problem`] in [`View`]: never as
//! the window going down.

pub mod audio;
pub mod demux;
pub mod sound;
pub mod video;

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use audio::AudioDecoder;
use demux::{Codec, Demuxer, Packet};
use sound::Sound;
pub use video::Picture;
use video::VideoDecoder;

/// A file to read from, as the player needs it.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A file read in pieces from any place: on this computer, or on a server over a connection.
pub trait Source: Send + Sync {
    fn size(&self) -> u64;
    /// Up to `buf.len()` bytes from `at`; fewer only at the end.
    fn read_at(&self, at: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// [`Source`] as [`Read`] and [`Seek`], a piece at a time: containers read a few bytes at once,
/// and a server's file is read in pieces this big.
pub struct Cursor {
    source: Arc<dyn Source>,
    at: u64,
    piece: Vec<u8>,
    piece_at: u64,
}

const PIECE: usize = 256 * 1024;

impl Cursor {
    pub fn new(source: Arc<dyn Source>) -> Cursor {
        Cursor { source, at: 0, piece: Vec::new(), piece_at: 0 }
    }
}

impl Read for Cursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size = self.source.size();
        if self.at >= size || buf.is_empty() {
            return Ok(0);
        }
        let inside = self.at >= self.piece_at && self.at < self.piece_at + self.piece.len() as u64;
        if !inside {
            // Big reads go straight through; small ones take a piece.
            if buf.len() >= PIECE {
                let n = self.source.read_at(self.at, buf)?;
                self.at += n as u64;
                return Ok(n);
            }
            let want = PIECE.min((size - self.at) as usize);
            self.piece.resize(want, 0);
            let n = self.source.read_at(self.at, &mut self.piece)?;
            self.piece.truncate(n);
            self.piece_at = self.at;
            if n == 0 {
                return Ok(0);
            }
        }
        let from = (self.at - self.piece_at) as usize;
        let n = buf.len().min(self.piece.len() - from);
        buf[..n].copy_from_slice(&self.piece[from..from + n]);
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Cursor {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let size = self.source.size() as i128;
        let at = match to {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => size + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.at) + i128::from(n),
        };
        if at < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"));
        }
        self.at = at.min(i128::from(u64::MAX)) as u64;
        Ok(self.at)
    }
}

/// Bytes in memory as a [`Source`] (the demo's video, tests).
impl Source for Vec<u8> {
    fn size(&self) -> u64 {
        self.len() as u64
    }
    fn read_at(&self, at: u64, buf: &mut [u8]) -> io::Result<usize> {
        let Some(rest) = usize::try_from(at).ok().and_then(|at| self.get(at..)) else { return Ok(0) };
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        Ok(n)
    }
}

/// Why a video doesn't play, in the words the preview says it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A kind of file or picture this player doesn't decode.
    Unsupported(String),
    /// Bigger than the preview plays.
    TooBig(String),
    /// Damaged, cut short, or unreadable.
    Broken(String),
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Unsupported(s) | Problem::TooBig(s) | Problem::Broken(s) => f.write_str(s),
        }
    }
}

/// Where pictures go, and who hears that something changed.
pub trait Screen: Send + Sync + 'static {
    /// The picture to show now. Called from the player's thread.
    fn show(&self, picture: Picture);
    /// [`Player::view`] changed: a new state, or time moved on (a few times a second while
    /// playing).
    fn changed(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Reading the file, up to its first picture.
    Opening,
    Paused,
    Playing,
    /// Played to the end; Play starts it again.
    Ended,
    /// It can't play: [`View::problem`] says why.
    Failed,
}

/// What the screen shows about a player.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub status: Status,
    pub problem: Option<Problem>,
    /// Seconds into the video.
    pub time: f64,
    pub duration: Option<f64>,
    /// The picture's size in pixels, once known.
    pub size: Option<[u32; 2]>,
    /// The video has sound this player plays (`None` until it's known).
    pub sound: Option<bool>,
    /// 0 to 1, and whether it's muted: what the screen set.
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Play the sound. Tests and headless runs play none, and keep to the computer's clock.
    pub sound: bool,
}

enum Cmd {
    Play,
    Pause,
    Seek(f64),
    Gain,
}

struct Shared {
    view: Mutex<View>,
    /// The player's thread has ended.
    done: AtomicBool,
}

impl Shared {
    fn view(&self) -> std::sync::MutexGuard<'_, View> {
        self.view.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn gain(&self) -> f32 {
        let v = self.view();
        if v.muted {
            0.0
        } else {
            v.volume
        }
    }
}

/// One video, playing or ready to. See the module's docs.
pub struct Player {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
}

impl Player {
    /// Starts reading `file` on a thread of its own; the first picture comes to `screen` when
    /// it's decoded, and the video waits, paused, for [`Player::play`].
    pub fn open(file: Box<dyn ReadSeek>, screen: Arc<dyn Screen>, options: Options) -> Player {
        let view = View { status: Status::Opening, problem: None, time: 0.0, duration: None, size: None, sound: None, volume: 1.0, muted: false };
        let shared = Arc::new(Shared { view: Mutex::new(view), done: AtomicBool::new(false) });
        let (tx, rx) = mpsc::channel();
        let (s, sc) = (shared.clone(), screen.clone());
        let spawned = std::thread::Builder::new().name("video".into()).spawn(move || run(file, s, rx, sc, options));
        if spawned.is_err() {
            fail(&shared, &*screen, Problem::Broken("This computer couldn't start the player.".into()));
            shared.done.store(true, Ordering::Release);
        }
        Player { shared, tx }
    }

    pub fn view(&self) -> View {
        self.shared.view().clone()
    }

    pub fn play(&self) {
        let _ = self.tx.send(Cmd::Play);
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }

    /// Play when paused or ended, pause when playing (Space, a click on the picture).
    pub fn toggle(&self) {
        match self.view().status {
            Status::Playing => self.pause(),
            Status::Paused | Status::Ended => self.play(),
            Status::Opening | Status::Failed => {}
        }
    }

    /// To `seconds` into the video; the picture there shows, playing or not.
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() {
            self.shared.view().time = seconds.max(0.0);
            let _ = self.tx.send(Cmd::Seek(seconds.max(0.0)));
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let mut v = self.shared.view();
        v.volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        v.muted = false;
        drop(v);
        let _ = self.tx.send(Cmd::Gain);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.view().muted = muted;
        let _ = self.tx.send(Cmd::Gain);
    }

    /// A way to see, after the player is gone too, whether its thread has ended.
    pub fn watch(&self) -> Watch {
        Watch(self.shared.clone())
    }
}

/// See [`Player::watch`].
pub struct Watch(Arc<Shared>);

impl Watch {
    /// The player's thread has ended: after the player was dropped, or it failed.
    pub fn stopped(&self) -> bool {
        self.0.done.load(Ordering::Acquire)
    }
}

fn fail(shared: &Shared, screen: &dyn Screen, problem: Problem) {
    {
        let mut v = shared.view();
        v.status = Status::Failed;
        v.problem = Some(problem);
    }
    screen.changed();
}

fn run(file: Box<dyn ReadSeek>, shared: Arc<Shared>, rx: Receiver<Cmd>, screen: Arc<dyn Screen>, options: Options) {
    let result = catch_unwind(AssertUnwindSafe(|| Engine::open(file, shared.clone(), screen.clone(), options).and_then(|mut e| e.run(&rx))));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(problem)) => fail(&shared, &*screen, problem),
        Err(_) => fail(&shared, &*screen, Problem::Broken("This video stopped the player: its pictures couldn't be decoded.".into())),
    }
    shared.done.store(true, Ordering::Release);
}

/// Pictures decoded ahead, at most: a few, and never more memory than this.
const AHEAD: usize = 8;
const AHEAD_BYTES: usize = 96 * 1024 * 1024;
/// Sound decoded ahead.
const SOUND_AHEAD: f64 = 0.6;
/// Video packets that may go in without a picture coming out, before the video is called
/// undecodable.
const NO_PICTURE: usize = 240;

/// What time it is in the video.
struct Clock {
    base: f64,
    wall: Instant,
    played: u64,
    by_sound: bool,
}

struct Engine {
    shared: Arc<Shared>,
    screen: Arc<dyn Screen>,
    demux: Demuxer,
    video: VideoDecoder,
    audio: Option<AudioDecoder>,
    sound: Option<Sound>,
    queue: VecDeque<Picture>,
    pictures: Vec<Picture>,
    pcm: Vec<f32>,
    /// The file has no more packets.
    eof: bool,
    playing: bool,
    clock: Clock,
    duration: Option<f64>,
    /// The last picture shown, and the quarter second last told.
    shown: f64,
    told: i64,
}

impl Engine {
    fn open(file: Box<dyn ReadSeek>, shared: Arc<Shared>, screen: Arc<dyn Screen>, options: Options) -> Result<Engine, Problem> {
        let demux = Demuxer::open(file)?;
        let tracks = demux.tracks().clone();
        let Some((codec, size)) = &tracks.video else { return Err(Problem::Unsupported("This file has no picture to play.".into())) };
        if u64::from(size[0]) * u64::from(size[1]) > video::MAX_PIXELS {
            return Err(Problem::TooBig(format!("This video is too big to play here ({} × {}, over 4096 × 2304).", size[0], size[1])));
        }
        let video = VideoDecoder::new(codec)?;
        // Sound that can't be played doesn't stop the picture.
        let audio = tracks.audio.as_ref().filter(|c| !matches!(c, Codec::Other(_))).and_then(|c| AudioDecoder::new(c, sound::RATE).ok());
        let sound = if options.sound && audio.is_some() { Sound::open() } else { None };
        {
            let mut v = shared.view();
            v.duration = tracks.duration;
            v.size = (size[0] > 0 && size[1] > 0).then_some(*size);
            v.sound = Some(audio.is_some());
        }
        let engine = Engine {
            shared,
            screen,
            demux,
            video,
            audio,
            sound,
            queue: VecDeque::new(),
            pictures: Vec::new(),
            pcm: Vec::new(),
            eof: false,
            playing: false,
            clock: Clock { base: 0.0, wall: Instant::now(), played: 0, by_sound: false },
            duration: tracks.duration,
            shown: 0.0,
            told: -1,
        };
        engine.apply_gain();
        Ok(engine)
    }

    fn run(&mut self, rx: &Receiver<Cmd>) -> Result<(), Problem> {
        // The first picture, shown paused.
        self.catch_up(0.0)?;
        self.set_status(Status::Paused);
        loop {
            let wait = if self.playing { self.wait() } else { Duration::from_secs(3600) };
            match rx.recv_timeout(wait) {
                Ok(cmd) => {
                    let mut cmds = vec![cmd];
                    cmds.extend(rx.try_iter());
                    // Of seeks in a row (a knob dragged), only the last matters.
                    let last_seek = cmds.iter().rposition(|c| matches!(c, Cmd::Seek(_)));
                    for (i, cmd) in cmds.into_iter().enumerate() {
                        match cmd {
                            Cmd::Seek(_) if Some(i) != last_seek => {}
                            cmd => self.handle(cmd)?,
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The player was dropped: the preview closed.
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            if self.playing {
                self.step()?;
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) -> Result<(), Problem> {
        match cmd {
            Cmd::Play if !self.playing => {
                if self.status() == Status::Ended {
                    self.seek(0.0)?;
                }
                self.playing = true;
                self.rebase(self.clock.base);
                if let Some(s) = &self.sound {
                    s.set_running(true);
                }
                self.set_status(Status::Playing);
            }
            Cmd::Pause if self.playing => {
                let t = self.time();
                self.playing = false;
                if let Some(s) = &self.sound {
                    s.set_running(false);
                }
                self.clock.base = t;
                self.publish_time(t, true);
                self.set_status(Status::Paused);
            }
            Cmd::Seek(t) => {
                self.seek(t)?;
                if self.status() == Status::Ended {
                    self.set_status(Status::Paused);
                }
            }
            Cmd::Gain => self.apply_gain(),
            Cmd::Play | Cmd::Pause => {}
        }
        Ok(())
    }

    fn apply_gain(&self) {
        if let Some(s) = &self.sound {
            s.set_gain(self.shared.gain());
        }
    }

    fn status(&self) -> Status {
        self.shared.view().status
    }

    fn set_status(&self, status: Status) {
        self.shared.view().status = status;
        self.screen.changed();
    }

    /// The time now, by the sound when it plays, else by this computer's clock.
    fn time(&self) -> f64 {
        if !self.playing {
            return self.clock.base;
        }
        match (&self.sound, self.clock.by_sound) {
            (Some(s), true) => self.clock.base + s.played().saturating_sub(self.clock.played) as f64 / f64::from(sound::RATE),
            _ => self.clock.base + self.clock.wall.elapsed().as_secs_f64(),
        }
    }

    fn rebase(&mut self, t: f64) {
        self.clock.base = t;
        self.clock.wall = Instant::now();
        self.clock.played = self.sound.as_ref().map_or(0, Sound::played);
        self.clock.by_sound = self.sound.is_some() && !self.eof;
    }

    /// Tells the screen the time, a few times a second (or now, `force`).
    fn publish_time(&mut self, t: f64, force: bool) {
        let t = match self.duration {
            Some(d) => t.clamp(0.0, d),
            None => t.max(0.0),
        };
        self.shared.view().time = t;
        let quarter = (t * 4.0) as i64;
        if force || quarter != self.told {
            self.told = quarter;
            self.screen.changed();
        }
    }

    /// Goes to `t`: the picture there shows, and the sound from there waits to play.
    fn seek(&mut self, t: f64) -> Result<(), Problem> {
        let t = match self.duration {
            Some(d) => t.clamp(0.0, d),
            None => t.max(0.0),
        };
        self.demux.seek(t)?;
        self.video.reset()?;
        if let Some(a) = &mut self.audio {
            a.reset();
        }
        if let Some(s) = &self.sound {
            s.clear();
        }
        self.queue.clear();
        self.eof = false;
        self.catch_up(t)?;
        self.rebase(t);
        self.publish_time(t, true);
        Ok(())
    }

    /// Reads and decodes from where the file is up to the picture at `t`, shows it, and keeps
    /// the sound from `t` on.
    fn catch_up(&mut self, t: f64) -> Result<(), Problem> {
        let mut last: Option<Picture> = None;
        let mut no_picture = 0;
        loop {
            let Some(packet) = self.demux.packet()? else {
                self.eof = true;
                self.video.finish(&mut self.pictures);
                for p in self.pictures.drain(..) {
                    if last.as_ref().is_none_or(|l| p.pts >= l.pts) {
                        last = Some(p);
                    }
                }
                break;
            };
            if !packet.video {
                // Sound from `t` on; a little before it is decoded (some codecs need what came
                // before) and cut off.
                if packet.pts + 0.5 >= t {
                    self.sound_packet(&packet, t);
                }
                continue;
            }
            self.decode(&packet)?;
            if self.pictures.is_empty() {
                no_picture += 1;
                if no_picture > NO_PICTURE {
                    return Err(Problem::Broken("This video's pictures couldn't be decoded.".into()));
                }
                continue;
            }
            no_picture = 0;
            // The picture to show is the last at or before `t` (the first, when the video
            // starts after it); those after it wait their turn.
            let mut found = false;
            for p in self.pictures.drain(..) {
                if p.pts <= t + 0.001 || last.is_none() {
                    found |= p.pts > t + 0.001;
                    last = Some(p);
                } else {
                    self.queue.push_back(p);
                    found = true;
                }
            }
            if found {
                break;
            }
        }
        let Some(first) = last else {
            return Err(Problem::Broken("This video has no pictures to show.".into()));
        };
        self.show(first);
        Ok(())
    }

    fn show(&mut self, p: Picture) {
        self.shown = p.pts;
        if self.duration.is_none_or(|d| p.pts > d) && self.eof {
            self.duration = Some(p.pts);
            self.shared.view().duration = Some(p.pts);
        }
        self.screen.show(p);
    }

    fn decode(&mut self, packet: &Packet) -> Result<(), Problem> {
        self.video.decode(&packet.data, packet.pts, packet.key, &mut self.pictures)
    }

    /// Decodes a sound packet into the speakers' queue, without what's before `from`.
    fn sound_packet(&mut self, packet: &Packet, from: f64) {
        let (Some(a), Some(s)) = (&mut self.audio, &self.sound) else { return };
        self.pcm.clear();
        a.decode(&packet.data, &mut self.pcm);
        let early = ((from - packet.pts) * f64::from(sound::RATE)).round().max(0.0) as usize * 2;
        s.push(&self.pcm[early.min(self.pcm.len())..]);
    }

    fn ahead_bytes(&self) -> usize {
        self.queue.iter().map(Picture::bytes).sum()
    }

    /// Wants more decoded: pictures or sound running low, and more in the file.
    fn hungry(&self) -> bool {
        if self.eof {
            return false;
        }
        let pictures = self.queue.len() < AHEAD && self.ahead_bytes() < AHEAD_BYTES;
        let sound = self.sound.as_ref().is_some_and(|s| s.queued() < SOUND_AHEAD);
        pictures || sound
    }

    /// How long to sleep before the next step: none while hungry, else until the next picture.
    fn wait(&self) -> Duration {
        if self.hungry() {
            return Duration::ZERO;
        }
        let next = self.queue.front().map_or(0.02, |p| p.pts - self.time());
        Duration::from_secs_f64(next.clamp(0.001, 0.02))
    }

    /// While playing: shows what's due, decodes ahead, and notices the end.
    fn step(&mut self) -> Result<(), Problem> {
        // The sound ran out before the picture: the computer's clock takes over.
        if self.clock.by_sound && self.eof && self.sound.as_ref().is_none_or(|s| s.queued() <= 0.0) {
            let t = self.time();
            self.rebase(t);
            self.clock.by_sound = false;
        }
        let now = self.time();
        let mut due = None;
        while self.queue.front().is_some_and(|p| p.pts <= now + 0.004) {
            due = self.queue.pop_front();
        }
        if let Some(p) = due {
            self.show(p);
        }
        // Decode ahead, for a moment at most, so commands are heard.
        let started = Instant::now();
        while self.hungry() && started.elapsed() < Duration::from_millis(25) {
            match self.demux.packet()? {
                Some(packet) if packet.video => {
                    self.decode(&packet)?;
                    let mut fresh = std::mem::take(&mut self.pictures);
                    for p in fresh.drain(..) {
                        // Out of order (a damaged stream): it goes where it belongs.
                        let at = self.queue.partition_point(|q| q.pts <= p.pts);
                        self.queue.insert(at, p);
                    }
                    self.pictures = fresh;
                    // Too much held (sound far behind in the file): the oldest go.
                    while self.ahead_bytes() > AHEAD_BYTES && self.queue.len() > 1 {
                        if let Some(p) = self.queue.pop_front() {
                            self.show(p);
                        }
                    }
                }
                Some(packet) => self.sound_packet(&packet, 0.0),
                None => {
                    self.eof = true;
                    let mut left = Vec::new();
                    self.video.finish(&mut left);
                    self.queue.extend(left);
                }
            }
        }
        let now = self.time();
        let sound_left = self.sound.as_ref().is_some_and(|s| s.queued() > 0.0) && self.clock.by_sound;
        if self.eof && self.queue.is_empty() && !sound_left {
            self.playing = false;
            if let Some(s) = &self.sound {
                s.set_running(false);
            }
            let end = self.duration.unwrap_or(self.shown).max(self.shown);
            self.clock.base = end;
            self.publish_time(end, true);
            self.set_status(Status::Ended);
            return Ok(());
        }
        self.publish_time(now, false);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
