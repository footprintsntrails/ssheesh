//! Pictures out of compressed packets: H.264 by OpenH264, VP9 by rusty_vp9. Each comes out as
//! NV12 (a full-size plane of brightness, then a half-size one of colour pairs), which the
//! window's GPU turns to colour as it draws.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use openh264::formats::YUVSource;

use crate::demux::Codec;
use crate::Problem;

/// The most pixels a picture may have: 4096 × 2304 (a little over 4K). Bigger is refused before
/// anything is decoded, so a file can't ask for more memory than a preview should take.
pub const MAX_PIXELS: u64 = 4096 * 2304;

/// One decoded picture, NV12, ready to show at `pts` seconds.
#[derive(Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// `width × height` bytes of brightness.
    pub y: Vec<u8>,
    /// `width × height / 2` bytes: blue-difference and red-difference, interleaved, one pair
    /// for each 2 × 2 pixels.
    pub uv: Vec<u8>,
    pub pts: f64,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({}×{} at {:.3}s)", self.width, self.height, self.pts)
    }
}

impl Picture {
    /// Memory it holds.
    pub fn bytes(&self) -> usize {
        self.y.len() + self.uv.len()
    }

    /// An NV12 picture from planar 4:2:0 planes (`y`, `u`, `v` with their strides), cut to
    /// even sides, as NV12 needs.
    fn from_planes(size: (usize, usize), planes: [&[u8]; 3], strides: [usize; 3], pts: f64) -> Option<Picture> {
        let (w, h) = (size.0 & !1, size.1 & !1);
        if w == 0 || h == 0 || (w * h) as u64 > MAX_PIXELS {
            return None;
        }
        let mut y = Vec::with_capacity(w * h);
        for row in 0..h {
            y.extend_from_slice(planes[0].get(row * strides[0]..row * strides[0] + w)?);
        }
        let (cw, ch) = (w / 2, h / 2);
        let mut uv = Vec::with_capacity(w * ch);
        for row in 0..ch {
            let u = planes[1].get(row * strides[1]..row * strides[1] + cw)?;
            let v = planes[2].get(row * strides[2]..row * strides[2] + cw)?;
            for (a, b) in u.iter().zip(v) {
                uv.push(*a);
                uv.push(*b);
            }
        }
        Some(Picture { width: w as u32, height: h as u32, y, uv, pts })
    }
}

pub enum VideoDecoder {
    H264(Box<H264>),
    Vp9(Box<rusty_vp9::Vp9Decoder>),
}

pub struct H264 {
    decoder: openh264::decoder::Decoder,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    nal_len: usize,
    /// The times of packets sent and not yet out as pictures. Pictures come out in the order
    /// they're shown, so each takes the earliest (B-frames come in another order).
    waiting: BinaryHeap<Reverse<i64>>,
}

fn h264() -> Result<openh264::decoder::Decoder, Problem> {
    use openh264::decoder::{Decoder, DecoderConfig, Flush};
    // Not flushed after each packet: pictures then come out in the order they're shown, B-frames
    // in their place, a few packets late.
    let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
    Decoder::with_api_config(openh264::OpenH264API::from_source(), config).map_err(|e| Problem::Broken(format!("The H.264 decoder didn't start: {e}")))
}

/// Seconds to microseconds, for ordering.
fn micros(t: f64) -> i64 {
    (t * 1e6).round() as i64
}

impl VideoDecoder {
    pub fn new(codec: &Codec) -> Result<VideoDecoder, Problem> {
        match codec {
            Codec::H264 { sps, pps, nal_len } => Ok(VideoDecoder::H264(Box::new(H264 { decoder: h264()?, sps: sps.clone(), pps: pps.clone(), nal_len: *nal_len, waiting: BinaryHeap::new() }))),
            Codec::Vp9 => Ok(VideoDecoder::Vp9(Box::default())),
            other => Err(Problem::Unsupported(format!("{} videos can't be played here.", other.name()))),
        }
    }

    /// Starts over, as after a seek: nothing decoded before carries on.
    pub fn reset(&mut self) -> Result<(), Problem> {
        match self {
            VideoDecoder::H264(h) => {
                h.decoder = h264()?;
                h.waiting.clear();
            }
            VideoDecoder::Vp9(d) => **d = rusty_vp9::Vp9Decoder::new(),
        }
        Ok(())
    }

    /// Decodes one packet; the pictures it finished come out (none, one, sometimes more).
    pub fn decode(&mut self, data: &[u8], pts: f64, key: bool, out: &mut Vec<Picture>) -> Result<(), Problem> {
        match self {
            VideoDecoder::H264(h) => h.decode(data, pts, key, out),
            VideoDecoder::Vp9(d) => {
                d.push(data, Some(micros(pts))).map_err(vp9_failed)?;
                loop {
                    match d.next_frame() {
                        Ok(f) => {
                            if let Some(p) = vp9_picture(&f) {
                                out.push(p);
                            }
                        }
                        Err(rusty_vp9::Error::Again | rusty_vp9::Error::Eof) => return Ok(()),
                        Err(e) => return Err(vp9_failed(e)),
                    }
                }
            }
        }
    }

    /// The pictures still held at the end of the file.
    pub fn finish(&mut self, out: &mut Vec<Picture>) {
        match self {
            VideoDecoder::H264(h) => {
                let H264 { decoder, waiting, .. } = &mut **h;
                if let Ok(left) = decoder.flush_remaining() {
                    for yuv in left {
                        let pts = waiting.pop().map_or(0.0, |Reverse(t)| t as f64 / 1e6);
                        out.extend(h264_picture(&yuv, pts));
                    }
                }
            }
            VideoDecoder::Vp9(d) => {
                d.flush();
                while let Ok(f) = d.next_frame() {
                    out.extend(vp9_picture(&f));
                }
            }
        }
    }
}

fn vp9_failed(e: rusty_vp9::Error) -> Problem {
    Problem::Broken(format!("This video's pictures couldn't be decoded: {e}"))
}

fn vp9_picture(f: &rusty_vp9::DecodedFrame) -> Option<Picture> {
    if f.subsampling_x != 1 || f.subsampling_y != 1 || f.planes.len() < 3 || f.strides.len() < 3 {
        return None;
    }
    let pts = f.pts.map_or(0.0, |t| t as f64 / 1e6);
    let size = (f.width as usize, f.height as usize);
    if f.bit_depth == 8 {
        return Picture::from_planes(size, [&f.planes[0], &f.planes[1], &f.planes[2]], [f.strides[0], f.strides[1], f.strides[2]], pts);
    }
    // 10 and 12 bits: little-endian pairs, down to 8.
    let shift = f.bit_depth.saturating_sub(8);
    let narrow = |p: &[u8]| p.as_chunks::<2>().0.iter().map(|b| (u16::from_le_bytes(*b) >> shift).min(255) as u8).collect::<Vec<u8>>();
    let planes = [narrow(&f.planes[0]), narrow(&f.planes[1]), narrow(&f.planes[2])];
    Picture::from_planes(size, [&planes[0], &planes[1], &planes[2]], [f.strides[0] / 2, f.strides[1] / 2, f.strides[2] / 2], pts)
}

fn h264_picture(yuv: &openh264::decoder::DecodedYUV<'_>, pts: f64) -> Option<Picture> {
    let (sy, su, sv) = yuv.strides();
    Picture::from_planes(yuv.dimensions(), [yuv.y(), yuv.u(), yuv.v()], [sy, su, sv], pts)
}

impl H264 {
    fn decode(&mut self, data: &[u8], pts: f64, key: bool, out: &mut Vec<Picture>) -> Result<(), Problem> {
        // Length-prefixed NAL units to Annex B's start codes, the settings before a key picture.
        let mut stream = Vec::with_capacity(data.len() + 64);
        if key {
            for set in self.sps.iter().chain(&self.pps) {
                stream.extend_from_slice(&[0, 0, 0, 1]);
                stream.extend_from_slice(set);
            }
        }
        let mut at = 0;
        while at + self.nal_len <= data.len() {
            let len = data[at..at + self.nal_len].iter().fold(0usize, |n, &b| (n << 8) | usize::from(b));
            at += self.nal_len;
            let Some(nal) = data.get(at..at.saturating_add(len)) else {
                return Err(Problem::Broken("A picture in this video is cut short.".into()));
            };
            stream.extend_from_slice(&[0, 0, 0, 1]);
            stream.extend_from_slice(nal);
            at += len;
        }
        self.waiting.push(Reverse(micros(pts)));
        // Enough held means the stream is wrong; don't keep growing.
        if self.waiting.len() > 64 {
            self.waiting.clear();
        }
        let H264 { decoder, waiting, .. } = self;
        match decoder.decode(&stream) {
            Ok(Some(yuv)) => {
                let pts = waiting.pop().map_or(pts, |Reverse(t)| t as f64 / 1e6);
                out.extend(h264_picture(&yuv, pts));
                Ok(())
            }
            Ok(None) => Ok(()),
            // OpenH264 says so on a damaged picture and goes on with the next.
            Err(_) => Ok(()),
        }
    }
}
