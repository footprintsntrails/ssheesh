//! The speakers: stereo samples go into a queue here, and the system's sound takes them from
//! it at its own pace (Core Audio and WASAPI through cpal; ALSA on Linux, which PulseAudio and
//! PipeWire both serve). How much it has taken is the player's clock, so the picture keeps to
//! the sound.
//!
//! No sound device, or none that opens, is not an error: the video plays silently on the
//! computer's clock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The rate everything is played at; the system converts it for the speakers when they differ.
pub const RATE: u32 = 48_000;

/// What the speakers' side and the player share.
#[derive(Default)]
pub(crate) struct Ring {
    queue: Mutex<VecDeque<f32>>,
    /// Stereo frames heard so far.
    played: AtomicU64,
    running: AtomicBool,
    /// The volume, as f32 bits; 0 while muted.
    gain: AtomicU32,
    closed: AtomicBool,
}

impl Ring {
    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Fills `out` (`channels` interleaved) from the queue; silence for what isn't there, or
    /// while paused. Returns the frames taken.
    fn take(&self, out: &mut [f32], channels: usize) -> usize {
        let frames = out.len() / channels.max(1);
        if !self.running.load(Ordering::Acquire) {
            out.fill(0.0);
            return 0;
        }
        let gain = self.gain();
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let n = frames.min(q.len() / 2);
        for (i, frame) in out.chunks_mut(channels.max(1)).enumerate() {
            if i < n {
                let (l, r) = (q.pop_front().unwrap_or(0.0), q.pop_front().unwrap_or(0.0));
                for (c, s) in frame.iter_mut().enumerate() {
                    *s = gain
                        * match (c, channels) {
                            (_, 1) => (l + r) * 0.5,
                            (0, _) => l,
                            (1, _) => r,
                            _ => 0.0,
                        };
                }
            } else {
                frame.fill(0.0);
            }
        }
        drop(q);
        n
    }
}

pub struct Sound {
    ring: Arc<Ring>,
    _out: platform::Out,
}

impl Sound {
    /// The system's default output, or `None` when there's none to play on.
    pub fn open() -> Option<Sound> {
        let ring = Arc::new(Ring::default());
        ring.gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        let out = platform::Out::open(ring.clone())?;
        Some(Sound { ring, _out: out })
    }

    pub fn push(&self, stereo: &[f32]) {
        self.ring.queue.lock().unwrap_or_else(|e| e.into_inner()).extend(stereo.iter().copied());
    }

    /// Seconds of sound waiting to be played.
    pub fn queued(&self) -> f64 {
        let samples = self.ring.queue.lock().unwrap_or_else(|e| e.into_inner()).len();
        samples as f64 / 2.0 / f64::from(RATE)
    }

    /// Frames heard since it opened.
    pub fn played(&self) -> u64 {
        self.ring.played.load(Ordering::Acquire)
    }

    pub fn set_running(&self, on: bool) {
        self.ring.running.store(on, Ordering::Release);
    }

    /// Drops what's waiting (a seek).
    pub fn clear(&self) {
        self.ring.queue.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub fn set_gain(&self, gain: f32) {
        self.ring.gain.store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.ring.closed.store(true, Ordering::Release);
        self.ring.running.store(false, Ordering::Release);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod platform {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    pub struct Out(cpal::Stream);

    impl Out {
        pub fn open(ring: Arc<Ring>) -> Option<Out> {
            let device = cpal::default_host().default_output_device()?;
            let channels = device.default_output_config().ok()?.channels().max(1);
            let config = cpal::StreamConfig { channels, sample_rate: cpal::SampleRate(RATE), buffer_size: cpal::BufferSize::Default };
            let ch = usize::from(channels);
            let stream = device
                .build_output_stream(
                    &config,
                    move |out: &mut [f32], _| {
                        let n = ring.take(out, ch);
                        ring.played.fetch_add(n as u64, Ordering::AcqRel);
                    },
                    |_| {},
                    None,
                )
                .ok()?;
            stream.play().ok()?;
            Some(Out(stream))
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    //! ALSA, from libasound.so.2 opened when a video first plays: the program needs neither
    //! its headers to build nor the library to start. Its "default" device is PulseAudio's or
    //! PipeWire's on a desktop, which convert the rate for the speakers.
    use super::*;
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
    use std::time::Duration;

    type Pcm = *mut c_void;

    struct Alsa {
        _lib: libloading::Library,
        open: unsafe extern "C" fn(*mut Pcm, *const c_char, c_int, c_int) -> c_int,
        set_params: unsafe extern "C" fn(Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
        writei: unsafe extern "C" fn(Pcm, *const c_void, c_ulong) -> c_long,
        recover: unsafe extern "C" fn(Pcm, c_int, c_int) -> c_int,
        delay: unsafe extern "C" fn(Pcm, *mut c_long) -> c_int,
        drop: unsafe extern "C" fn(Pcm) -> c_int,
        prepare: unsafe extern "C" fn(Pcm) -> c_int,
        close: unsafe extern "C" fn(Pcm) -> c_int,
    }

    const PLAYBACK: c_int = 0;
    const FLOAT_LE: c_int = 14;
    const RW_INTERLEAVED: c_int = 3;
    /// What ALSA holds ahead, in microseconds.
    const LATENCY: c_uint = 80_000;

    impl Alsa {
        fn load() -> Option<Alsa> {
            // SAFETY: libasound is the system's ALSA library; each symbol is looked up by its
            // documented name and used with its documented C signature (alsa/pcm.h).
            unsafe {
                let lib = libloading::Library::new("libasound.so.2").ok()?;
                macro_rules! sym {
                    ($name:literal) => {
                        *lib.get($name).ok()?
                    };
                }
                Some(Alsa {
                    open: sym!(b"snd_pcm_open\0"),
                    set_params: sym!(b"snd_pcm_set_params\0"),
                    writei: sym!(b"snd_pcm_writei\0"),
                    recover: sym!(b"snd_pcm_recover\0"),
                    delay: sym!(b"snd_pcm_delay\0"),
                    drop: sym!(b"snd_pcm_drop\0"),
                    prepare: sym!(b"snd_pcm_prepare\0"),
                    close: sym!(b"snd_pcm_close\0"),
                    _lib: lib,
                })
            }
        }
    }

    /// The thread writing to ALSA ends when the ring closes.
    pub struct Out;

    struct Handle(Pcm);
    // SAFETY: the handle is used by the one thread it's moved to, and closed there.
    unsafe impl Send for Handle {}

    impl Out {
        pub fn open(ring: Arc<Ring>) -> Option<Out> {
            let alsa = Alsa::load()?;
            let mut pcm: Pcm = std::ptr::null_mut();
            // SAFETY: documented ALSA calls on a handle this function owns until it's moved.
            unsafe {
                if (alsa.open)(&mut pcm, c"default".as_ptr(), PLAYBACK, 0) < 0 || pcm.is_null() {
                    return None;
                }
                if (alsa.set_params)(pcm, FLOAT_LE, RW_INTERLEAVED, 2, RATE, 1, LATENCY) < 0 {
                    (alsa.close)(pcm);
                    return None;
                }
            }
            let handle = Handle(pcm);
            std::thread::Builder::new()
                .name("video-sound".into())
                .spawn(move || {
                    let handle = handle;
                    write(&alsa, handle.0, &ring);
                    // SAFETY: the handle is open and nothing else uses it.
                    unsafe { (alsa.close)(handle.0) };
                })
                .ok()?;
            Some(Out)
        }
    }

    fn write(alsa: &Alsa, pcm: Pcm, ring: &Ring) {
        const CHUNK: usize = 1024;
        let mut buf = vec![0f32; CHUNK * 2];
        // Frames handed to ALSA, and whether it's running.
        let mut written: u64 = 0;
        let mut flowing = true;
        // What was last handed to ALSA, to put back in the queue when a pause drops it.
        let mut recent: VecDeque<f32> = VecDeque::new();
        let keep = (RATE as usize / 2) * 2;
        while !ring.closed.load(Ordering::Acquire) {
            let running = ring.running.load(Ordering::Acquire);
            // SAFETY (each call below): documented ALSA calls on the open handle.
            if !running {
                if flowing {
                    // Paused: what ALSA still holds is dropped, isn't counted as heard, and
                    // goes back to the front of the queue to be heard after the pause.
                    let mut delay: c_long = 0;
                    if unsafe { (alsa.delay)(pcm, &mut delay) } == 0 {
                        let held = (delay.max(0) as u64).min(written).min(recent.len() as u64 / 2);
                        written -= held;
                        let mut q = ring.queue.lock().unwrap_or_else(|e| e.into_inner());
                        for s in recent.iter().rev().take(held as usize * 2) {
                            q.push_front(*s);
                        }
                    }
                    recent.clear();
                    unsafe { (alsa.drop)(pcm) };
                    ring.played.store(written, Ordering::Release);
                    flowing = false;
                }
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            if !flowing {
                unsafe { (alsa.prepare)(pcm) };
                flowing = true;
            }
            let waiting = ring.queue.lock().unwrap_or_else(|e| e.into_inner()).len() / 2;
            if waiting == 0 {
                std::thread::sleep(Duration::from_millis(5));
                heard(alsa, pcm, ring, written);
                continue;
            }
            let n = ring.take(&mut buf[..CHUNK.min(waiting) * 2], 2);
            // As taken, before the volume: what's put back plays at the volume then.
            // (`take` applied the gain; undo it by keeping the queue's own samples instead.)
            let gain = ring.gain();
            recent.extend(buf[..n * 2].iter().map(|s| if gain > 0.0 { s / gain } else { 0.0 }));
            while recent.len() > keep {
                recent.pop_front();
            }
            let mut sent = 0usize;
            while sent < n && !ring.closed.load(Ordering::Acquire) {
                let r = unsafe { (alsa.writei)(pcm, buf[sent * 2..].as_ptr().cast(), (n - sent) as c_ulong) };
                if r < 0 {
                    if unsafe { (alsa.recover)(pcm, r as c_int, 1) } < 0 {
                        return;
                    }
                    continue;
                }
                sent += r as usize;
                written += r as u64;
            }
            heard(alsa, pcm, ring, written);
        }
    }

    /// What's been heard: what was written, less what ALSA still holds.
    fn heard(alsa: &Alsa, pcm: Pcm, ring: &Ring, written: u64) {
        let mut delay: c_long = 0;
        // SAFETY: a documented ALSA call on the open handle.
        let heard = if unsafe { (alsa.delay)(pcm, &mut delay) } == 0 { written.saturating_sub(delay.max(0) as u64) } else { written };
        ring.played.fetch_max(heard, Ordering::AcqRel);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::*;
    pub struct Out;
    impl Out {
        pub fn open(_: Arc<Ring>) -> Option<Out> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speakers_get_the_queue_paused_silent_and_at_its_volume() {
        let ring = Ring::default();
        ring.gain.store(0.5f32.to_bits(), Ordering::Relaxed);
        ring.queue.lock().unwrap().extend([1.0, -1.0, 0.5, 0.5]);
        let mut out = [9.0f32; 6];
        // Paused: silence, nothing taken.
        assert_eq!(ring.take(&mut out, 2), 0);
        assert_eq!(out, [0.0; 6]);
        ring.running.store(true, Ordering::Release);
        // Two frames there, three asked for: the third is silence.
        assert_eq!(ring.take(&mut out, 2), 2);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.25, 0.0, 0.0]);
        // One speaker: both sides, halved.
        ring.queue.lock().unwrap().extend([1.0, 0.0]);
        let mut mono = [0.0f32; 1];
        assert_eq!(ring.take(&mut mono, 1), 1);
        assert_eq!(mono, [0.25]);
    }
}
