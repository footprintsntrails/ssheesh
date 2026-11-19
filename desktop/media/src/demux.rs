//! A video file's packets in the order they're stored, each with its track and presentation
//! time. MP4 and MOV (`Mp4`) and Matroska and WebM (`Mkv`); anything else says what it is.
//!
//! A file is untrusted: nothing here believes a size it hasn't checked against the file and
//! the limits below, and every failure is a [`Problem`], never a panic of its own.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

use crate::{Problem, ReadSeek};

/// The most of an MP4's index (its `moov`) read, as 0.10's player did (`MAX_INDEX`).
pub const MAX_INDEX: u64 = 64 * 1024 * 1024;
/// The biggest packet read: a picture this big compressed isn't one this preview plays.
pub const MAX_PACKET: u64 = 32 * 1024 * 1024;

/// What a track holds, and what its decoder needs to start.
#[derive(Debug, Clone, PartialEq)]
pub enum Codec {
    /// H.264. `avcc`: packets are NAL units each after its length in `nal_len` bytes (MP4,
    /// Matroska's `V_MPEG4/ISO/AVC`), with the parameter sets kept apart.
    H264 { sps: Vec<Vec<u8>>, pps: Vec<Vec<u8>>, nal_len: usize },
    Vp9,
    /// AAC, with its AudioSpecificConfig when the file has one.
    Aac { config: Option<Vec<u8>>, rate: u32, channels: u16 },
    /// Vorbis: its identification and setup headers, one after the other.
    Vorbis { headers: Vec<u8> },
    /// Opus: `skip` samples (at 48 kHz) at the start are the encoder's, not the sound's.
    Opus { channels: u16, skip: u32 },
    /// Something this player doesn't decode, by a name people know.
    Other(String),
}

impl Codec {
    /// The name the preview says it by.
    pub fn name(&self) -> &str {
        match self {
            Codec::H264 { .. } => "H.264",
            Codec::Vp9 => "VP9",
            Codec::Aac { .. } => "AAC",
            Codec::Vorbis { .. } => "Vorbis",
            Codec::Opus { .. } => "Opus",
            Codec::Other(name) => name,
        }
    }
}

/// A file's tracks: the first picture track and the first sound track.
#[derive(Debug, Clone, PartialEq)]
pub struct Tracks {
    pub video: Option<(Codec, [u32; 2])>,
    pub audio: Option<Codec>,
    /// In seconds, when the file says.
    pub duration: Option<f64>,
}

/// One packet: compressed, of the picture track (`video`) or the sound track.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub video: bool,
    /// Presentation time, in seconds.
    pub pts: f64,
    pub key: bool,
    pub data: Vec<u8>,
}

pub enum Demuxer {
    Mp4(Box<Mp4>),
    Mkv(Box<Mkv>),
}

impl Demuxer {
    /// Reads a file's header and index. Its first bytes tell what kind of file it is.
    pub fn open(mut file: Box<dyn ReadSeek>) -> Result<Demuxer, Problem> {
        let size = file.seek(SeekFrom::End(0)).map_err(read_failed)?;
        file.seek(SeekFrom::Start(0)).map_err(read_failed)?;
        let mut head = [0u8; 12];
        let got = read_up_to(&mut file, &mut head).map_err(read_failed)?;
        let head = &head[..got];
        file.seek(SeekFrom::Start(0)).map_err(read_failed)?;
        if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            return Ok(Demuxer::Mkv(Box::new(Mkv::open(file)?)));
        }
        let boxed = |kind: &[u8]| head.len() >= 8 && &head[4..8] == kind;
        if ["ftyp", "moov", "mdat", "wide", "free", "skip"].iter().any(|k| boxed(k.as_bytes())) {
            return Ok(Demuxer::Mp4(Box::new(Mp4::open(file, size)?)));
        }
        let kind = if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"AVI ") {
            "AVI"
        } else if head.starts_with(b"OggS") {
            "Ogg"
        } else {
            return Err(Problem::Unsupported("This file isn't a video this preview knows (MP4 or WebM).".into()));
        };
        Err(Problem::Unsupported(format!("{kind} videos can't be played here, only MP4 and WebM.")))
    }

    pub fn tracks(&self) -> &Tracks {
        match self {
            Demuxer::Mp4(m) => &m.tracks,
            Demuxer::Mkv(m) => &m.tracks,
        }
    }

    /// The next packet of the two tracks, in the file's order. `None` at the end.
    pub fn packet(&mut self) -> Result<Option<Packet>, Problem> {
        match self {
            Demuxer::Mp4(m) => m.packet(),
            Demuxer::Mkv(m) => m.packet(),
        }
    }

    /// Goes to the last picture a decoder can start from at or before `t` (seconds): the
    /// next packets are from there on. Returns that picture's time.
    pub fn seek(&mut self, t: f64) -> Result<f64, Problem> {
        match self {
            Demuxer::Mp4(m) => m.seek(t),
            Demuxer::Mkv(m) => m.seek(t),
        }
    }
}

fn read_failed(e: std::io::Error) -> Problem {
    Problem::Broken(format!("The file couldn't be read: {e}"))
}

fn read_up_to(r: &mut dyn ReadSeek, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

// ---- MP4

/// Where one sample is and when it's shown.
#[derive(Debug, Clone, Copy)]
struct Sample {
    offset: u64,
    size: u32,
    /// Decode time, in the track's timescale.
    dts: i64,
    /// Presentation time, in seconds.
    pts: f64,
    key: bool,
}

struct Track {
    samples: Vec<Sample>,
    /// The next sample to read.
    next: usize,
    timescale: f64,
}

pub struct Mp4 {
    file: Box<dyn ReadSeek>,
    tracks: Tracks,
    video: Option<Track>,
    audio: Option<Track>,
}

/// The sizes of an MP4's top-level boxes, to find its index without reading the rest.
fn moov_size(file: &mut dyn ReadSeek, size: u64) -> Result<Option<u64>, Problem> {
    let mut at = 0u64;
    while at + 8 <= size {
        file.seek(SeekFrom::Start(at)).map_err(read_failed)?;
        let mut h = [0u8; 16];
        let got = read_up_to(file, &mut h[..8]).map_err(read_failed)?;
        if got < 8 {
            break;
        }
        let mut len = u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]]));
        if len == 1 {
            read_up_to(file, &mut h[8..16]).map_err(read_failed)?;
            len = u64::from_be_bytes([h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15]]);
        } else if len == 0 {
            len = size - at;
        }
        if &h[4..8] == b"moov" {
            return Ok(Some(len));
        }
        if len < 8 {
            break;
        }
        at = at.saturating_add(len);
    }
    Ok(None)
}

impl Mp4 {
    fn open(mut file: Box<dyn ReadSeek>, size: u64) -> Result<Mp4, Problem> {
        match moov_size(&mut *file, size)? {
            None => return Err(Problem::Broken("This video has no index: it may not have finished being written.".into())),
            Some(len) if len > MAX_INDEX => return Err(Problem::TooBig("This video's index is too big to read here.".into())),
            Some(_) => {}
        }
        file.seek(SeekFrom::Start(0)).map_err(read_failed)?;
        let reader = mp4::Mp4Reader::read_header(&mut file, size).map_err(|e| Problem::Broken(format!("This video couldn't be read: {e}")))?;
        let mut tracks = Tracks { video: None, audio: None, duration: None };
        let (mut video, mut audio) = (None, None);
        let mut other_video = None;
        for trak in &reader.moov.traks {
            let handler = trak.mdia.hdlr.handler_type.value;
            let stsd = &trak.mdia.minf.stbl.stsd;
            let timescale = trak.mdia.mdhd.timescale;
            if timescale == 0 {
                continue;
            }
            let shift = trak.edts.as_ref().and_then(|e| e.elst.as_ref()).and_then(|l| l.entries.iter().find(|e| e.media_time != u64::MAX && e.media_time as i64 >= 0)).map_or(0, |e| e.media_time as i64);
            if &handler == b"vide" && tracks.video.is_none() {
                let codec = if let Some(avc) = &stsd.avc1 {
                    let c = &avc.avcc;
                    Codec::H264 {
                        sps: c.sequence_parameter_sets.iter().map(|n| n.bytes.clone()).collect(),
                        pps: c.picture_parameter_sets.iter().map(|n| n.bytes.clone()).collect(),
                        nal_len: usize::from(c.length_size_minus_one & 3) + 1,
                    }
                } else if stsd.vp09.is_some() {
                    Codec::Vp9
                } else if stsd.hev1.is_some() {
                    Codec::Other("HEVC".into())
                } else {
                    other_video.get_or_insert(());
                    continue;
                };
                let size = match (&stsd.avc1, &stsd.vp09) {
                    (Some(a), _) => [u32::from(a.width), u32::from(a.height)],
                    (_, Some(v)) => [u32::from(v.width), u32::from(v.height)],
                    _ => [u32::from(trak.tkhd.width.value()), u32::from(trak.tkhd.height.value())],
                };
                video = Some(Track { samples: samples(&table!(trak.mdia.minf.stbl), shift, timescale)?, next: 0, timescale: f64::from(timescale) });
                tracks.video = Some((codec, size));
            } else if &handler == b"soun" && tracks.audio.is_none() {
                let Some(mp4a) = &stsd.mp4a else {
                    tracks.audio = Some(Codec::Other("this sound".into()));
                    continue;
                };
                let rate = u32::from(mp4a.samplerate.value());
                let config = mp4a.esds.as_ref().map(|e| {
                    let d = &e.es_desc.dec_config.dec_specific;
                    let v = (u16::from(d.profile) << 11) | (u16::from(d.freq_index & 0xF) << 7) | (u16::from(d.chan_conf & 0xF) << 3);
                    v.to_be_bytes().to_vec()
                });
                audio = Some(Track { samples: samples(&table!(trak.mdia.minf.stbl), shift, timescale)?, next: 0, timescale: f64::from(timescale) });
                tracks.audio = Some(Codec::Aac { config, rate, channels: mp4a.channelcount });
            }
            let seconds = trak.mdia.mdhd.duration as f64 / f64::from(timescale);
            if seconds.is_finite() && seconds > 0.0 {
                tracks.duration = Some(tracks.duration.map_or(seconds, |d: f64| d.max(seconds)));
            }
        }
        if tracks.video.is_none() {
            return Err(match other_video {
                Some(()) => Problem::Unsupported("This video's picture is in a format that can't be played here.".into()),
                None => Problem::Unsupported("This file has no picture to play.".into()),
            });
        }
        // An audio track that can't be decoded doesn't stop the picture.
        if !matches!(tracks.audio, Some(Codec::Aac { .. })) {
            audio = None;
        }
        drop(reader);
        Ok(Mp4 { file, tracks, video, audio })
    }

    fn packet(&mut self) -> Result<Option<Packet>, Problem> {
        // Whichever track's next sample is first in time; the file stores them about so.
        let pick = |t: &Option<Track>| t.as_ref().and_then(|t| t.samples.get(t.next).map(|s| s.dts as f64 / t.timescale));
        let (v, a) = (pick(&self.video), pick(&self.audio));
        let video = match (v, a) {
            (None, None) => return Ok(None),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(v), Some(a)) => v <= a,
        };
        let track = if video { self.video.as_mut() } else { self.audio.as_mut() };
        let Some(track) = track else { return Ok(None) };
        let sample = track.samples[track.next];
        track.next += 1;
        if u64::from(sample.size) > MAX_PACKET {
            return Err(Problem::TooBig("A picture in this video is too big to play here.".into()));
        }
        self.file.seek(SeekFrom::Start(sample.offset)).map_err(read_failed)?;
        let mut data = vec![0u8; sample.size as usize];
        self.file.read_exact(&mut data).map_err(|_| Problem::Broken("This video ends before its last picture: it may not have finished copying.".into()))?;
        Ok(Some(Packet { video, pts: sample.pts, key: sample.key, data }))
    }

    fn seek(&mut self, t: f64) -> Result<f64, Problem> {
        let Some(video) = self.video.as_mut() else { return Ok(0.0) };
        // The last key picture shown at or before `t`.
        let mut at = 0;
        for (i, s) in video.samples.iter().enumerate() {
            if s.key && s.pts <= t + 1e-6 {
                at = i;
            }
            if s.dts as f64 / video.timescale > t + 10.0 {
                break;
            }
        }
        video.next = at;
        let from = video.samples.get(at).map_or(0.0, |s| s.dts as f64 / video.timescale);
        if let Some(audio) = self.audio.as_mut() {
            audio.next = audio.samples.partition_point(|s| (s.dts as f64 / audio.timescale) < from);
        }
        Ok(video.samples.get(at).map_or(0.0, |s| s.pts))
    }
}

/// A track's sample tables, out of the mp4 crate's boxes (whose types it doesn't export).
struct Table<'a> {
    /// Every sample's size, or 0 and `sizes`.
    sample_size: u32,
    count: u32,
    sizes: &'a [u32],
    chunks: Vec<u64>,
    /// (first chunk, samples per chunk).
    runs: Vec<(u32, u32)>,
    /// (samples, duration of each).
    stts: Vec<(u32, u32)>,
    /// (samples, offset of each).
    ctts: Vec<(u32, i32)>,
    /// The samples a decoder can start from, counted from 1; `None`: all.
    stss: Option<&'a [u32]>,
}

macro_rules! table {
    ($stbl:expr) => {{
        let stbl = &$stbl;
        Table {
            sample_size: stbl.stsz.sample_size,
            count: stbl.stsz.sample_count,
            sizes: &stbl.stsz.sample_sizes,
            chunks: match (&stbl.stco, &stbl.co64) {
                (Some(c), _) => c.entries.iter().map(|&o| u64::from(o)).collect(),
                (_, Some(c)) => c.entries.clone(),
                _ => Vec::new(),
            },
            runs: stbl.stsc.entries.iter().map(|e| (e.first_chunk, e.samples_per_chunk)).collect(),
            stts: stbl.stts.entries.iter().map(|e| (e.sample_count, e.sample_delta)).collect(),
            ctts: stbl.ctts.as_ref().map(|c| c.entries.iter().map(|e| (e.sample_count, e.sample_offset)).collect()).unwrap_or_default(),
            stss: stbl.stss.as_ref().map(|s| s.entries.as_slice()),
        }
    }};
}
use table;

/// Every sample of a track: where it is (stsc, stco/co64, stsz), when (stts, ctts, the edit
/// list's `shift`) and which a decoder can start from (stss; all, when there's none).
fn samples(t: &Table, shift: i64, timescale: u32) -> Result<Vec<Sample>, Problem> {
    let broken = || Problem::Broken("This video's index doesn't add up.".into());
    let count = t.count as usize;
    let sizes = t.sizes;
    if t.sample_size == 0 && sizes.len() != count {
        return Err(broken());
    }
    let chunks = &t.chunks;
    if chunks.is_empty() && count > 0 {
        return Err(broken());
    }
    // Each sample's place: its chunk's offset plus the sizes before it in that chunk.
    let mut out = Vec::with_capacity(count.min(4 << 20));
    let runs = &t.runs;
    let mut sample = 0usize;
    for (r, &(first, per_chunk)) in runs.iter().enumerate() {
        let first = first.max(1) as usize;
        let last = runs.get(r + 1).map_or(chunks.len(), |n| (n.0 as usize).saturating_sub(1).min(chunks.len()));
        for chunk in first..=last {
            let Some(&base) = chunks.get(chunk - 1) else { break };
            let mut offset = base;
            for _ in 0..per_chunk {
                if sample >= count {
                    break;
                }
                let size = if t.sample_size == 0 { sizes[sample] } else { t.sample_size };
                out.push(Sample { offset, size, dts: 0, pts: 0.0, key: true });
                offset = offset.saturating_add(u64::from(size));
                sample += 1;
            }
        }
    }
    if out.len() != count {
        return Err(broken());
    }
    // Times.
    let mut dts = 0i64;
    let mut i = 0;
    for &(n, delta) in &t.stts {
        for _ in 0..n {
            let Some(s) = out.get_mut(i) else { break };
            s.dts = dts;
            dts = dts.saturating_add(i64::from(delta));
            i += 1;
        }
    }
    let mut offsets: Vec<i64> = Vec::new();
    for &(n, offset) in &t.ctts {
        for _ in 0..n {
            if offsets.len() >= count {
                break;
            }
            offsets.push(i64::from(offset));
        }
    }
    let scale = f64::from(timescale);
    for (i, s) in out.iter_mut().enumerate() {
        let shown = s.dts.saturating_add(offsets.get(i).copied().unwrap_or(0)).saturating_sub(shift);
        s.pts = shown as f64 / scale;
    }
    if let Some(stss) = t.stss {
        for s in out.iter_mut() {
            s.key = false;
        }
        for &n in stss {
            if let Some(s) = (n as usize).checked_sub(1).and_then(|i| out.get_mut(i)) {
                s.key = true;
            }
        }
    }
    Ok(out)
}

// ---- Matroska and WebM

pub struct Mkv {
    file: matroska_demuxer::MatroskaFile<Jumping>,
    tracks: Tracks,
    video_track: u64,
    audio_track: Option<u64>,
    /// Nanoseconds a timestamp counts.
    scale: f64,
    frame: matroska_demuxer::Frame,
    /// Packets read past a seek's key picture, before it.
    queued: VecDeque<Packet>,
    /// Where the demuxer reads next, set here to seek.
    jump: Arc<Mutex<Option<u64>>>,
    /// (time, cluster's place in the file): the file's Cues, then its first cluster.
    cues: Vec<(u64, u64)>,
    first_cluster: Option<u64>,
}

/// The file under the demuxer, which a seek moves to a cluster of its own finding: the
/// demuxer's own seek misreads where clusters are in files ffmpeg writes (CueRelativePosition
/// taken from the first cluster), so the Cues are read here instead.
pub struct Jumping {
    file: Box<dyn ReadSeek>,
    jump: Arc<Mutex<Option<u64>>>,
}

impl Jumping {
    fn jumped(&mut self) -> std::io::Result<()> {
        if let Some(at) = self.jump.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.file.seek(SeekFrom::Start(at))?;
        }
        Ok(())
    }
}

impl Read for Jumping {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.jumped()?;
        self.file.read(buf)
    }
}

impl Seek for Jumping {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.jumped()?;
        self.file.seek(to)
    }
}

/// The most of a file's Cues read.
const MAX_CUES: u64 = 16 * 1024 * 1024;

fn ebml_id(r: &mut dyn ReadSeek) -> Option<u32> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).ok()?;
    let len = b[0].leading_zeros() as usize + 1;
    if len > 4 {
        return None;
    }
    let mut id = u32::from(b[0]);
    for _ in 1..len {
        r.read_exact(&mut b).ok()?;
        id = (id << 8) | u32::from(b[0]);
    }
    Some(id)
}

/// An element's size; `Some(None)` when it's unknown (a file being written).
fn ebml_size(r: &mut dyn ReadSeek) -> Option<Option<u64>> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).ok()?;
    let len = b[0].leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let mut v = u64::from(b[0]) & (0xFF >> len);
    let mut all_ones = v == 0xFF >> len;
    for _ in 1..len {
        r.read_exact(&mut b).ok()?;
        v = (v << 8) | u64::from(b[0]);
        all_ones &= b[0] == 0xFF;
    }
    Some((!all_ones).then_some(v))
}

/// An element in `bytes` at `at`: (id, data start, data end).
fn ebml_at(bytes: &[u8], at: usize) -> Option<(u32, usize, usize)> {
    let mut c = std::io::Cursor::new(bytes.get(at..)?);
    let id = ebml_id(&mut c)?;
    let size = ebml_size(&mut c)??;
    let start = at + c.position() as usize;
    let end = start.checked_add(usize::try_from(size).ok()?)?;
    (end <= bytes.len()).then_some((id, start, end))
}

fn ebml_uint(bytes: &[u8]) -> u64 {
    bytes.iter().take(8).fold(0, |n, &b| (n << 8) | u64::from(b))
}

/// The children of a master element's data: (id, data).
fn ebml_children(bytes: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((id, start, end)) = ebml_at(bytes, at) {
        out.push((id, &bytes[start..end]));
        at = end;
    }
    out
}

/// A Matroska file's Cues as (time, cluster's place in the file), and where its first
/// cluster is. Empty when it has none (it's then read from the start to seek).
fn read_cues(file: &mut dyn ReadSeek) -> (Vec<(u64, u64)>, Option<u64>) {
    let mut cues = Vec::new();
    let mut first_cluster = None;
    let found = (|| -> Option<()> {
        file.seek(SeekFrom::Start(0)).ok()?;
        (ebml_id(file)? == 0x1A45_DFA3).then_some(())?;
        let header = ebml_size(file)??;
        file.seek(SeekFrom::Current(i64::try_from(header).ok()?)).ok()?;
        (ebml_id(file)? == 0x1853_8067).then_some(())?;
        ebml_size(file)?;
        let segment = file.stream_position().ok()?;
        let mut at = segment;
        let mut cues_at = None;
        // The segment's first elements: its SeekHead says where the Cues are.
        for _ in 0..32 {
            file.seek(SeekFrom::Start(at)).ok()?;
            let id = ebml_id(file)?;
            let size = ebml_size(file)?;
            let data = file.stream_position().ok()?;
            match id {
                0x114D_9B74 => {
                    let size = size?.min(1 << 20);
                    let mut head = vec![0u8; size as usize];
                    file.read_exact(&mut head).ok()?;
                    for (id, seek) in ebml_children(&head) {
                        let fields = ebml_children(seek);
                        let target = fields.iter().find(|(i, _)| *i == 0x53AB).map(|f| f.1);
                        let place = fields.iter().find(|(i, _)| *i == 0x53AC).map(|f| ebml_uint(f.1));
                        if id == 0x4DBB && target == Some(&[0x1C, 0x53, 0xBB, 0x6B]) {
                            cues_at = place.map(|p| segment + p);
                        }
                    }
                }
                0x1C53_BB6B => cues_at = Some(at),
                0x1F43_B675 => {
                    first_cluster.get_or_insert(at);
                    break;
                }
                _ => {}
            }
            at = data.checked_add(size?)?;
        }
        file.seek(SeekFrom::Start(cues_at?)).ok()?;
        (ebml_id(file)? == 0x1C53_BB6B).then_some(())?;
        let size = ebml_size(file)??;
        if size > MAX_CUES {
            return None;
        }
        let mut all = vec![0u8; size as usize];
        file.read_exact(&mut all).ok()?;
        for (id, point) in ebml_children(&all) {
            if id != 0xBB {
                continue;
            }
            let fields = ebml_children(point);
            let time = fields.iter().find(|(i, _)| *i == 0xB3).map(|f| ebml_uint(f.1));
            let place = fields.iter().find(|(i, _)| *i == 0xB7).and_then(|p| ebml_children(p.1).into_iter().find(|(i, _)| *i == 0xF1)).map(|f| ebml_uint(f.1));
            if let (Some(time), Some(place)) = (time, place) {
                cues.push((time, segment.saturating_add(place)));
            }
        }
        Some(())
    })();
    if found.is_none() {
        cues.clear();
    }
    cues.sort_unstable();
    (cues, first_cluster)
}

impl Mkv {
    fn open(mut file: Box<dyn ReadSeek>) -> Result<Mkv, Problem> {
        use matroska_demuxer::{MatroskaFile, TrackType};
        let (cues, first_cluster) = read_cues(&mut *file);
        file.seek(SeekFrom::Start(0)).map_err(read_failed)?;
        let jump = Arc::new(Mutex::new(None));
        let file = MatroskaFile::open(Jumping { file, jump: jump.clone() }).map_err(|e| Problem::Broken(format!("This video couldn't be read: {e}")))?;
        let scale = file.info().timestamp_scale().get() as f64;
        let duration = file.info().duration().map(|d| d * scale / 1e9).filter(|d| d.is_finite() && *d > 0.0);
        let mut tracks = Tracks { video: None, audio: None, duration };
        let (mut video_track, mut audio_track) = (None, None);
        for t in file.tracks() {
            if t.content_encodings().is_some_and(|e| !e.is_empty()) {
                // Compressed or encrypted tracks: not for this preview.
                continue;
            }
            match t.track_type() {
                TrackType::Video if video_track.is_none() => {
                    let Some(v) = t.video() else { continue };
                    let size = [v.pixel_width().get().min(u64::from(u32::MAX)) as u32, v.pixel_height().get().min(u64::from(u32::MAX)) as u32];
                    let codec = match t.codec_id() {
                        "V_VP9" => Codec::Vp9,
                        "V_MPEG4/ISO/AVC" => match t.codec_private().and_then(avcc) {
                            Some(c) => c,
                            None => Codec::Other("H.264 without its settings".into()),
                        },
                        "V_VP8" => Codec::Other("VP8".into()),
                        "V_AV1" => Codec::Other("AV1".into()),
                        "V_MPEGH/ISO/HEVC" => Codec::Other("HEVC".into()),
                        id => Codec::Other(id.trim_start_matches("V_").to_owned()),
                    };
                    video_track = Some(t.track_number().get());
                    tracks.video = Some((codec, size));
                }
                TrackType::Audio if audio_track.is_none() => {
                    let Some(a) = t.audio() else { continue };
                    let channels = a.channels().get().min(8) as u16;
                    let codec = match t.codec_id() {
                        "A_OPUS" => {
                            let head = t.codec_private().unwrap_or_default();
                            let skip = head.get(10..12).map_or(0, |b| u32::from(u16::from_le_bytes([b[0], b[1]])));
                            Codec::Opus { channels, skip }
                        }
                        "A_VORBIS" => match t.codec_private().and_then(vorbis_headers) {
                            Some(headers) => Codec::Vorbis { headers },
                            None => Codec::Other("Vorbis without its settings".into()),
                        },
                        "A_AAC" => Codec::Aac { config: t.codec_private().map(<[u8]>::to_vec), rate: a.sampling_frequency() as u32, channels },
                        id => Codec::Other(id.trim_start_matches("A_").to_owned()),
                    };
                    audio_track = Some(t.track_number().get());
                    tracks.audio = Some(codec);
                }
                _ => {}
            }
        }
        let Some(video_track) = video_track else {
            return Err(Problem::Unsupported("This file has no picture to play.".into()));
        };
        if matches!(tracks.audio, Some(Codec::Other(_))) {
            audio_track = None;
        }
        Ok(Mkv { file, tracks, video_track, audio_track, scale, frame: Default::default(), queued: VecDeque::new(), jump, cues, first_cluster })
    }

    fn packet(&mut self) -> Result<Option<Packet>, Problem> {
        if let Some(p) = self.queued.pop_front() {
            return Ok(Some(p));
        }
        loop {
            let more = self.file.next_frame(&mut self.frame).map_err(|e| Problem::Broken(format!("This video couldn't be read on: {e}")))?;
            if !more {
                return Ok(None);
            }
            let video = self.frame.track == self.video_track;
            if !video && Some(self.frame.track) != self.audio_track {
                continue;
            }
            if self.frame.data.len() as u64 > MAX_PACKET {
                return Err(Problem::TooBig("A picture in this video is too big to play here.".into()));
            }
            let pts = self.frame.timestamp as f64 * self.scale / 1e9;
            let key = self.frame.is_keyframe.unwrap_or(false);
            return Ok(Some(Packet { video, pts, key, data: std::mem::take(&mut self.frame.data) }));
        }
    }

    fn seek(&mut self, t: f64) -> Result<f64, Problem> {
        // To the cluster the Cues name at or before the time (the first, when there are
        // none); the key picture is found by reading on, and what came before it is dropped.
        let ts = ((t.max(0.0) * 1e9) / self.scale) as u64;
        self.queued.clear();
        let Some(cluster) = self.cues.iter().rev().find(|c| c.0 <= ts).map(|c| c.1).or(self.first_cluster) else {
            return Err(Problem::Broken("This video can't be played from there: it has no index.".into()));
        };
        // The demuxer's own seek, to the start, only to forget what it held; then the jump.
        let _ = self.file.seek(0);
        *self.jump.lock().unwrap_or_else(|e| e.into_inner()) = Some(cluster);
        let mut kept: VecDeque<Packet> = VecDeque::new();
        let mut key_at = None;
        while let Some(p) = self.packet()? {
            if p.video && p.pts > t + 1e-6 && key_at.is_some() {
                kept.push_back(p);
                break;
            }
            if p.video && p.key {
                kept.clear();
                key_at = Some(p.pts);
            }
            if key_at.is_some() {
                kept.push_back(p);
            }
        }
        self.queued = kept;
        Ok(key_at.unwrap_or(t))
    }
}

/// H.264's settings from Matroska's CodecPrivate (an `avcC` record).
fn avcc(b: &[u8]) -> Option<Codec> {
    let nal_len = usize::from(*b.get(4)? & 3) + 1;
    let mut at = 5;
    let take = |at: &mut usize, b: &[u8]| -> Option<Vec<u8>> {
        let n = usize::from(u16::from_be_bytes([*b.get(*at)?, *b.get(*at + 1)?]));
        let out = b.get(*at + 2..*at + 2 + n)?.to_vec();
        *at += 2 + n;
        Some(out)
    };
    let sps_count = usize::from(*b.get(at)? & 0x1F);
    at += 1;
    let mut sps = Vec::new();
    for _ in 0..sps_count {
        sps.push(take(&mut at, b)?);
    }
    let pps_count = usize::from(*b.get(at)?);
    at += 1;
    let mut pps = Vec::new();
    for _ in 0..pps_count {
        pps.push(take(&mut at, b)?);
    }
    Some(Codec::H264 { sps, pps, nal_len })
}

/// Vorbis's identification and setup headers from Matroska's CodecPrivate (three packets,
/// Xiph-laced), without the comments between them.
fn vorbis_headers(b: &[u8]) -> Option<Vec<u8>> {
    if *b.first()? != 2 {
        return None;
    }
    let mut at = 1;
    let mut lace = || -> Option<usize> {
        let mut n = 0usize;
        loop {
            let v = *b.get(at)?;
            at += 1;
            n = n.checked_add(usize::from(v))?;
            if v != 255 {
                return Some(n);
            }
        }
    };
    let (ident, comments) = (lace()?, lace()?);
    let ident_at = at;
    let setup_at = ident_at.checked_add(ident)?.checked_add(comments)?;
    let mut out = b.get(ident_at..ident_at + ident)?.to_vec();
    out.extend_from_slice(b.get(setup_at..)?);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_webm_is_found_by_its_cues() {
        let bytes = include_bytes!("../fixtures/clip.webm").to_vec();
        let (cues, first) = read_cues(&mut std::io::Cursor::new(bytes.clone()));
        assert!(!cues.is_empty() && first.is_some(), "{cues:?} {first:?}");
        // Each cue names a cluster.
        for (_, at) in &cues {
            assert_eq!(&bytes[*at as usize..*at as usize + 4], &[0x1F, 0x43, 0xB6, 0x75]);
        }
        let mut d = Demuxer::open(Box::new(std::io::Cursor::new(bytes))).unwrap();
        // The clip has a key picture each second.
        assert_eq!(d.seek(1.5).unwrap(), 1.0);
        let p = d.packet().unwrap().unwrap();
        assert!(p.video && p.key && p.pts == 1.0, "{p:?}");
        assert_eq!(d.seek(0.3).unwrap(), 0.0);
    }

    #[test]
    fn what_isnt_mp4_or_webm_says_what_it_is() {
        let open = |bytes: &[u8]| match Demuxer::open(Box::new(std::io::Cursor::new(bytes.to_vec()))) {
            Err(p) => p.to_string(),
            Ok(_) => "opened".into(),
        };
        assert_eq!(open(b"RIFF\0\0\0\0AVI LIST"), "AVI videos can't be played here, only MP4 and WebM.");
        assert_eq!(open(b"OggS\0\0\0\0\0\0\0\0"), "Ogg videos can't be played here, only MP4 and WebM.");
        assert_eq!(open(b"hello"), "This file isn't a video this preview knows (MP4 or WebM).");
        assert_eq!(open(b""), "This file isn't a video this preview knows (MP4 or WebM).");
        // An MP4 cut before its index.
        assert_eq!(open(b"\0\0\0\x10ftypisom\0\0\0\0\0\0\0\x08mdat"), "This video has no index: it may not have finished being written.");
        // An index claiming more than this preview reads.
        let mut big = b"\0\0\0\x10ftypisom\0\0\0\0".to_vec();
        big.extend_from_slice(&[0x10, 0, 0, 0]);
        big.extend_from_slice(b"moov");
        assert_eq!(open(&big), "This video's index is too big to read here.");
    }

    #[test]
    fn matroska_settings_come_apart() {
        let rec = [1, 100, 0, 31, 0xFF, 0xE1, 0, 3, 0x67, 1, 2, 1, 0, 2, 0x68, 9];
        assert_eq!(avcc(&rec), Some(Codec::H264 { sps: vec![vec![0x67, 1, 2]], pps: vec![vec![0x68, 9]], nal_len: 4 }));
        assert_eq!(avcc(&rec[..10]), None);
        let laced = [2, 3, 2, 1, 2, 3, 9, 9, 7, 7, 7];
        assert_eq!(vorbis_headers(&laced), Some(vec![1, 2, 3, 7, 7, 7]));
        assert_eq!(vorbis_headers(&[2, 255]), None);
    }
}
