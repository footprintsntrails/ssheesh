//! The player on two made-up clips (2 s, 160 × 90, a test pattern and a 440 Hz tone; no
//! metadata): `fixtures/clip.mp4` (H.264 High with B-frames, AAC) and `fixtures/clip.webm`
//! (VP9, Opus). Without sound: tests keep to the computer's clock and make no noise.
//!
//! Made with ffmpeg's test sources:
//!
//! ```text
//! ffmpeg -f lavfi -i "testsrc2=size=160x90:rate=25:duration=2" -f lavfi -i "sine=frequency=440:duration=2:sample_rate=44100" \
//!   -c:v libx264 -profile:v high -bf 2 -g 25 -pix_fmt yuv420p -bsf:v filter_units=remove_types=6 -c:a aac -b:a 32k -ac 1 \
//!   -map_metadata -1 -fflags +bitexact -flags:v +bitexact -flags:a +bitexact -movflags +faststart clip.mp4
//! ffmpeg -f lavfi -i "testsrc2=size=160x90:rate=25:duration=2" -f lavfi -i "sine=frequency=440:duration=2:sample_rate=48000" \
//!   -c:v libvpx-vp9 -g 25 -b:v 100k -c:a libopus -b:a 24k -ac 1 -map_metadata -1 -fflags +bitexact \
//!   -flags:v +bitexact -flags:a +bitexact clip.webm
//! ```

use super::*;
use std::sync::Condvar;

const MP4: &[u8] = include_bytes!("../fixtures/clip.mp4");
const WEBM: &[u8] = include_bytes!("../fixtures/clip.webm");

/// A screen that keeps what it was shown.
#[derive(Default)]
struct Seen {
    shown: Mutex<Vec<(u32, u32, f64)>>,
    changes: Mutex<u64>,
    news: Condvar,
}

impl Screen for Seen {
    fn show(&self, p: Picture) {
        assert_eq!(p.y.len(), (p.width * p.height) as usize);
        assert_eq!(p.uv.len(), (p.width * p.height / 2) as usize);
        self.shown.lock().unwrap().push((p.width, p.height, p.pts));
        self.changed();
    }
    fn changed(&self) {
        *self.changes.lock().unwrap() += 1;
        self.news.notify_all();
    }
}

impl Seen {
    fn last(&self) -> Option<(u32, u32, f64)> {
        self.shown.lock().unwrap().last().copied()
    }
    fn count(&self) -> usize {
        self.shown.lock().unwrap().len()
    }
}

fn open(bytes: &[u8]) -> (Player, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let file = Cursor::new(Arc::new(bytes.to_vec()));
    (Player::open(Box::new(file), seen.clone(), Options { sound: false }), seen)
}

/// Waits (5 s at most) until `ok` holds of the player's view.
fn until(player: &Player, seen: &Seen, what: &str, ok: impl Fn(&View) -> bool) -> View {
    let end = Instant::now() + Duration::from_secs(5);
    let mut guard = seen.changes.lock().unwrap();
    loop {
        let view = player.view();
        if ok(&view) {
            return view;
        }
        assert!(Instant::now() < end, "never {what}: {view:?}");
        guard = seen.news.wait_timeout(guard, Duration::from_millis(20)).unwrap().0;
    }
}

fn plays(bytes: &[u8], sound: &str) {
    let (player, seen) = open(bytes);
    // Opened: the first picture shows, paused at the start.
    let view = until(&player, &seen, "ready", |v| v.status == Status::Paused);
    assert_eq!(view.size, Some([160, 90]));
    assert_eq!(view.sound, Some(true), "{sound}");
    assert!(view.duration.is_some_and(|d| (d - 2.0).abs() < 0.1), "{view:?}");
    assert_eq!(seen.last().map(|(w, h, t)| (w, h, t < 0.1)), Some((160, 90, true)));

    // Plays: pictures come, time goes on.
    player.play();
    until(&player, &seen, "past half a second", |v| v.status == Status::Playing && v.time > 0.5);
    assert!(seen.count() > 5);

    // Paused, it stays where it is.
    player.pause();
    let paused = until(&player, &seen, "paused", |v| v.status == Status::Paused);
    let shown = seen.count();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(player.view().time, paused.time);
    assert_eq!(seen.count(), shown);

    // A seek shows the picture there, paused or not.
    player.seek(1.5);
    until(&player, &seen, "at 1.5 s", |v| (v.time - 1.5).abs() < 0.01 && seen.last().is_some_and(|(_, _, t)| (t - 1.5).abs() < 0.05));
    assert_eq!(player.view().status, Status::Paused);
    player.seek(0.2);
    until(&player, &seen, "back at 0.2 s", |_| seen.last().is_some_and(|(_, _, t)| (t - 0.2).abs() < 0.05));

    // Played to the end, it says so; Play starts it again.
    player.seek(1.8);
    player.play();
    let ended = until(&player, &seen, "ended", |v| v.status == Status::Ended);
    assert!((ended.time - 2.0).abs() < 0.1, "{ended:?}");
    player.toggle();
    until(&player, &seen, "playing from the start", |v| v.status == Status::Playing && v.time < 1.0);

    // Volume and mute are the screen's to say.
    player.set_volume(0.3);
    player.set_muted(true);
    let view = player.view();
    assert_eq!((view.volume, view.muted), (0.3, true));

    // Closed: its thread ends, mid-play.
    let watch = player.watch();
    drop(player);
    let end = Instant::now() + Duration::from_secs(5);
    while !watch.stopped() {
        assert!(Instant::now() < end, "the player's thread didn't stop");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_mp4_plays_pauses_seeks_ends_and_stops_when_closed() {
    plays(MP4, "AAC");
}

#[test]
fn a_webm_plays_pauses_seeks_ends_and_stops_when_closed() {
    plays(WEBM, "Opus");
}

#[test]
fn what_cant_play_says_why_and_never_takes_the_player_down() {
    let failed = |bytes: Vec<u8>| {
        let (player, seen) = open(&bytes);
        let view = until(&player, &seen, "failed", |v| v.status == Status::Failed);
        assert!(player.watch().stopped() || { std::thread::sleep(Duration::from_millis(100)); player.watch().stopped() });
        view.problem.map(|p| p.to_string()).unwrap_or_default()
    };
    assert_eq!(failed(b"RIFF\0\0\0\0AVI LIST".to_vec()), "AVI videos can't be played here, only MP4 and WebM.");
    assert_eq!(failed(b"not a video at all".to_vec()), "This file isn't a video this preview knows (MP4 or WebM).");
    // Cut short: the index is there, the pictures aren't.
    let cut = failed(MP4[..MP4.len() / 3].to_vec());
    assert!(cut.starts_with("This video"), "{cut}");
    // Scrambled after the header: whatever the decoders make of it, the player says so or
    // plays what it can, and the test's own thread is still here.
    for seed in 0..8u32 {
        for clip in [MP4, WEBM] {
            let mut bytes = clip.to_vec();
            let mut x = 0x9E37_79B9u32.wrapping_mul(seed + 1);
            for b in bytes.iter_mut().skip(clip.len() / 2) {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if x.is_multiple_of(7) {
                    *b = x as u8;
                }
            }
            let (player, seen) = open(&bytes);
            player.play();
            until(&player, &seen, "ended or failed", |v| matches!(v.status, Status::Failed | Status::Ended | Status::Playing));
        }
    }
}

#[test]
fn a_picture_too_big_is_refused_before_decoding() {
    // The clip with its declared size grown past the limit (the avc1 entry's width and height).
    let mut bytes = MP4.to_vec();
    // The sample entry, inside stsd (the first "avc1" is a brand in ftyp).
    let stsd = bytes.windows(4).position(|w| w == b"stsd").expect("an stsd box");
    let at = stsd + bytes[stsd..].windows(4).position(|w| w == b"avc1").expect("an avc1 entry");
    // Width and height, 24 bytes after the entry's type.
    bytes[at + 28..at + 30].copy_from_slice(&8192u16.to_be_bytes());
    bytes[at + 30..at + 32].copy_from_slice(&8192u16.to_be_bytes());
    let (player, seen) = open(&bytes);
    let view = until(&player, &seen, "refused", |v| v.status == Status::Failed);
    assert_eq!(view.problem, Some(Problem::TooBig("This video is too big to play here (8192 × 8192, over 4096 × 2304).".into())));
    assert_eq!(seen.count(), 0);
}

/// Every picture of a clip, as decoded straight through.
fn decode_all(clip: &[u8]) -> Vec<Picture> {
    let mut d = demux::Demuxer::open(Box::new(std::io::Cursor::new(clip.to_vec()))).unwrap();
    let codec = d.tracks().video.clone().unwrap().0;
    let mut v = video::VideoDecoder::new(&codec).unwrap();
    let mut out = Vec::new();
    while let Some(p) = d.packet().unwrap() {
        if p.video {
            v.decode(&p.data, p.pts, p.key, &mut out).unwrap();
        }
    }
    v.finish(&mut out);
    out
}

#[test]
fn every_picture_comes_out_once_in_the_order_its_shown() {
    for clip in [MP4, WEBM] {
        let pictures = decode_all(clip);
        let times: Vec<i64> = pictures.iter().map(|p| (p.pts * 1000.0).round() as i64).collect();
        // 2 s at 25 a second, B-frames (the MP4's) put back in their place.
        assert_eq!(times, (0..50).map(|i| i * 40).collect::<Vec<_>>());
        // Each a new picture: the test pattern moves.
        assert!(pictures.windows(2).all(|w| w[0].y != w[1].y));
    }
}
