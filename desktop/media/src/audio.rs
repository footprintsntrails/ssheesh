//! Sound out of compressed packets (AAC, Vorbis, Opus), as stereo at the rate the speakers
//! play, ready for [`crate::sound`].

use symphonia_core::audio::{AudioBufferRef, SampleBuffer};
use symphonia_core::codecs::{CodecParameters, Decoder as _, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_VORBIS};
use symphonia_core::formats::Packet as SymPacket;

use crate::demux::Codec;
use crate::Problem;

/// The rates of sound played: from telephone sound to the most studio recordings use.
const MIN_RATE: u32 = 4_000;
const MAX_RATE: u32 = 384_000;

pub struct AudioDecoder {
    kind: Kind,
    /// The decoded sound's rate and channels, as it comes out.
    rate: u32,
    channels: usize,
    out: Resampler,
    /// Opus: samples (per channel) still to drop at the start.
    skip: usize,
}

enum Kind {
    Symphonia(Box<dyn symphonia_core::codecs::Decoder>, Option<SampleBuffer<f32>>),
    Opus(Box<opuscule::Decoder>, Vec<f32>),
}

impl AudioDecoder {
    /// A decoder for `codec`, putting out stereo at `out_rate`.
    pub fn new(codec: &Codec, out_rate: u32) -> Result<AudioDecoder, Problem> {
        let options = DecoderOptions::default();
        let unsupported = |what: &str| Problem::Unsupported(format!("This video's sound ({what}) can't be played here."));
        let (kind, rate, channels, skip) = match codec {
            Codec::Aac { config, rate, channels } => {
                let mut params = CodecParameters::new();
                params.for_codec(CODEC_TYPE_AAC).with_sample_rate((*rate).max(1));
                if let Some(c) = symphonia_core::audio::Channels::from_bits(((1u64 << (*channels).clamp(1, 8)) - 1) as u32) {
                    params.with_channels(c);
                }
                if let Some(config) = config {
                    params.with_extra_data(config.clone().into_boxed_slice());
                }
                let d = symphonia_codec_aac::AacDecoder::try_new(&params, &options).map_err(|_| unsupported("AAC"))?;
                let rate = d.codec_params().sample_rate.unwrap_or(*rate);
                let channels = d.codec_params().channels.map_or(usize::from(*channels), |c| c.count());
                (Kind::Symphonia(Box::new(d), None), rate, channels, 0)
            }
            Codec::Vorbis { headers } => {
                let mut params = CodecParameters::new();
                params.for_codec(CODEC_TYPE_VORBIS).with_extra_data(headers.clone().into_boxed_slice());
                let d = symphonia_codec_vorbis::VorbisDecoder::try_new(&params, &options).map_err(|_| unsupported("Vorbis"))?;
                let rate = d.codec_params().sample_rate.unwrap_or(48_000);
                let channels = d.codec_params().channels.map_or(2, |c| c.count());
                (Kind::Symphonia(Box::new(d), None), rate, channels, 0)
            }
            Codec::Opus { channels, skip } => {
                let layout = match channels {
                    1 => opuscule::Channels::Mono,
                    2 => opuscule::Channels::Stereo,
                    _ => return Err(unsupported("Opus surround")),
                };
                let d = opuscule::Decoder::new(opuscule::SampleRate::Hz48000, layout);
                // The longest Opus packet: 120 ms at 48 kHz.
                (Kind::Opus(Box::new(d), vec![0.0; 5760 * usize::from(*channels)]), 48_000, usize::from(*channels), *skip as usize)
            }
            other => return Err(unsupported(other.name())),
        };
        // A rate the file made up: at 1 Hz, every packet would come out 48,000 times longer.
        if !(MIN_RATE..=MAX_RATE).contains(&rate) || channels == 0 {
            return Err(unsupported(codec.name()));
        }
        Ok(AudioDecoder { kind, rate, channels, out: Resampler::new(rate, out_rate), skip })
    }

    /// After a seek: what's mid-way is dropped.
    pub fn reset(&mut self) {
        match &mut self.kind {
            Kind::Symphonia(d, _) => d.reset(),
            Kind::Opus(..) => {}
        }
        self.out.reset();
        self.skip = 0;
    }

    /// Decodes one packet, adding stereo samples at the output rate to `out`. A damaged
    /// packet is skipped: a moment of silence, not the end of the sound.
    pub fn decode(&mut self, data: &[u8], out: &mut Vec<f32>) {
        let mut pcm: Vec<f32> = Vec::new();
        match &mut self.kind {
            Kind::Symphonia(d, buf) => {
                let Ok(decoded) = d.decode(&SymPacket::new_from_slice(0, 0, 0, data)) else { return };
                let frames = decoded.frames();
                let spec = *decoded.spec();
                let fits = buf.as_ref().is_some_and(|b| b.capacity() >= frames * spec.channels.count());
                if !fits {
                    *buf = Some(SampleBuffer::new(frames.max(1) as u64, spec));
                }
                let Some(buf) = buf.as_mut() else { return };
                copy(decoded, buf);
                self.channels = spec.channels.count().max(1);
                pcm.extend_from_slice(buf.samples());
            }
            Kind::Opus(d, scratch) => {
                let Ok(n) = d.decode(Some(data), scratch, false) else { return };
                pcm.extend_from_slice(&scratch[..n * self.channels]);
            }
        }
        if self.skip > 0 {
            let drop = (self.skip * self.channels).min(pcm.len());
            pcm.drain(..drop);
            self.skip -= drop / self.channels;
        }
        self.out.push(&pcm, self.channels, out);
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}

fn copy(decoded: AudioBufferRef<'_>, buf: &mut SampleBuffer<f32>) {
    buf.copy_interleaved_ref(decoded);
}

/// Any number of channels at one rate to stereo at another, by straight lines between
/// samples: plenty for a preview, and nothing it reads can make it slow.
pub struct Resampler {
    step: f64,
    /// Where the next output sample falls, in input samples after `last`.
    at: f64,
    /// The input's last stereo sample, the line's start for the next packet.
    last: [f32; 2],
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        Resampler { step: f64::from(from.max(1)) / f64::from(to.max(1)), at: 0.0, last: [0.0; 2] }
    }

    pub fn reset(&mut self) {
        self.at = 0.0;
        self.last = [0.0; 2];
    }

    /// `pcm`: `channels` interleaved. Out: stereo interleaved.
    pub fn push(&mut self, pcm: &[f32], channels: usize, out: &mut Vec<f32>) {
        if channels == 0 {
            return;
        }
        let stereo = |i: usize| -> [f32; 2] {
            let f = &pcm[i * channels..(i + 1) * channels];
            match channels {
                1 => [f[0], f[0]],
                // Front left and right (and centre, where there is one, in both).
                2 => [f[0], f[1]],
                _ => [f[0] + 0.7 * f[2], f[1] + 0.7 * f[2]],
            }
        };
        let frames = pcm.len() / channels;
        if frames == 0 {
            return;
        }
        // Sample i of this packet sits at i + 1 after `last`.
        while self.at < frames as f64 {
            let i = self.at.floor() as usize;
            let frac = (self.at - i as f64) as f32;
            let a = if i == 0 { self.last } else { stereo(i - 1) };
            let b = stereo(i);
            out.push((a[0] + (b[0] - a[0]) * frac).clamp(-1.0, 1.0));
            out.push((a[1] + (b[1] - a[1]) * frac).clamp(-1.0, 1.0));
            self.at += self.step;
        }
        self.at -= frames as f64;
        self.last = stereo(frames - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_comes_out_stereo_at_the_speakers_rate() {
        // A second of mono at 44.1 kHz is a second of stereo at 48 kHz, give or take a sample.
        let mut r = Resampler::new(44_100, 48_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            r.push(&[0.5; 4410], 1, &mut out);
        }
        let frames = out.len() / 2;
        assert!((47_999..=48_001).contains(&frames), "{frames}");
        assert!(out[100..].iter().all(|s| (*s - 0.5).abs() < 1e-6));
        // Six channels: left and right, with the centre in both.
        let mut r = Resampler::new(48_000, 48_000);
        let mut out = Vec::new();
        r.push(&[0.1, 0.2, 0.5, 0.0, 0.0, 0.0, 0.1, 0.2, 0.5, 0.0, 0.0, 0.0], 6, &mut out);
        assert_eq!(out.len(), 4);
        assert!((out[2] - 0.45).abs() < 1e-6 && (out[3] - 0.55).abs() < 1e-6);
    }

    #[test]
    fn a_rate_a_file_made_up_is_refused() {
        // An MP4 can say its AAC is at 1 Hz, and with no settings of its own that's believed:
        // each packet would come out 48,000 times longer.
        let codec = |rate| Codec::Aac { config: None, rate, channels: 2 };
        for rate in [1, 3_999, 400_000] {
            assert!(matches!(AudioDecoder::new(&codec(rate), 48_000), Err(Problem::Unsupported(_))), "{rate}");
        }
        assert!(AudioDecoder::new(&codec(44_100), 48_000).is_ok());
    }
}
