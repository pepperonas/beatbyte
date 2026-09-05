//! The m4a/mp3 decode-offset audit (plan `docs/plans/ai-song-graph-upgrade.md`, L0).
//!
//! Writes a click track — one full-scale sample at exactly 1.000,
//! 2.000 … 10.000 s — as a WAV, then decodes every file named on the
//! command line through the game's analysis path
//! ([`beatbyte_audio::decode_file`]; playback constructs the very same
//! `rodio::Decoder`) and reports where the clicks land: the peak of
//! the first click, and the cross-correlation lag of the whole
//! decode against the original. A lossy codec smears a click over
//! its transform window, so the lag is the number to trust; the peak
//! is there to see the smear.
//!
//! ```bash
//! cargo run -p beatbyte-audio --example click_offset -- --write /tmp/click.wav
//! afconvert -f m4af -d aac /tmp/click.wav /tmp/click-apple.m4a
//! ffmpeg -i /tmp/click.wav -c:a aac /tmp/click-ffmpeg.m4a
//! lame -b 128 /tmp/click.wav /tmp/click.mp3
//! cargo run -p beatbyte-audio --example click_offset -- /tmp/click.wav /tmp/click-apple.m4a …
//! ```

use std::path::{Path, PathBuf};

use beatbyte_audio::decode::{AudioData, decode_file, write_wav_mono16};

const RATE: u32 = 44_100;

/// One full-scale sample at every whole second from 1 s up to and
/// including `clicks`, in a track `length_s` long. The test fixtures
/// use the 4-second, 3-click variant (`--short`), the audit the long
/// one.
fn click_track(length_s: u32, clicks: u32) -> AudioData {
    let mut samples = vec![0.0f32; (RATE * length_s) as usize];
    for k in 1..=clicks {
        samples[(k * RATE) as usize] = 1.0;
    }
    AudioData::from_mono(samples, RATE)
}

/// Lag (in samples of `reference`) at which `decoded` best matches
/// `reference`, searched over ±`span`. Positive = the decode is LATE
/// (its content sits further into the file).
fn best_lag(reference: &[f32], decoded: &[f32], span: i64) -> (i64, f64) {
    let mut best = (0i64, f64::MIN);
    for lag in -span..=span {
        let mut acc = 0.0f64;
        for (i, &r) in reference.iter().enumerate() {
            let j = i as i64 + lag;
            if j >= 0 && (j as usize) < decoded.len() {
                acc += f64::from(r) * f64::from(decoded[j as usize]);
            }
        }
        if acc > best.1 {
            best = (lag, acc);
        }
    }
    best
}

/// The PLAYBACK path: the same decoder, but appended to a rodio
/// `Player` on a headless mixer and pulled through it — everything
/// the music thread does short of the device. Returns the mono
/// samples the mixer produced at the reference rate.
fn through_player(path: &Path, length_s: u32) -> Option<Vec<f32>> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = rodio::Decoder::try_from(file).ok()?;
    let channels = core::num::NonZero::<u16>::MIN;
    let rate = core::num::NonZero::new(RATE)?;
    let (mixer, mut source) = rodio::mixer::mixer(channels, rate);
    let player = rodio::Player::connect_new(&mixer);
    player.append(decoder);
    player.play();
    let wanted = (RATE * (length_s + 1)) as usize;
    let mut out = Vec::with_capacity(wanted);
    for _ in 0..wanted {
        match source.next() {
            Some(sample) => out.push(sample),
            None => break,
        }
    }
    player.stop();
    Some(out)
}

fn report(path: &Path, reference: &AudioData, length_s: u32) {
    let audio = match decode_file(path) {
        Ok(audio) => audio,
        Err(error) => {
            println!("{}: cannot decode: {error}", path.display());
            return;
        }
    };
    let rate = audio.sample_rate();
    // The first click sits at 1.000 s; look for the peak in 0.9..1.1 s.
    let from = (f64::from(rate) * 0.9) as usize;
    let to = ((f64::from(rate) * 1.1) as usize).min(audio.samples().len());
    let (peak_at, peak) =
        audio.samples()[from..to]
            .iter()
            .enumerate()
            .fold((0usize, 0.0f32), |acc, (i, &s)| {
                if s.abs() > acc.1 {
                    (from + i, s.abs())
                } else {
                    acc
                }
            });
    let peak_ms = (peak_at as f64 / f64::from(rate) - 1.0) * 1000.0;
    let (lag, _) = best_lag(reference.samples(), audio.samples(), 4000);
    let lag_ms = lag as f64 / f64::from(rate) * 1000.0;
    println!(
        "{}: rate {rate} Hz, {} samples ({:.3} s); first click peak at sample {peak_at} \
         ({peak_ms:+.2} ms from 1.000 s, amplitude {peak:.2}); best lag vs original {lag:+} \
         samples ({lag_ms:+.2} ms)",
        path.display(),
        audio.samples().len(),
        audio.duration_s()
    );
    if let Some(played) = through_player(path, length_s) {
        let (lag, _) = best_lag(reference.samples(), &played, 4000);
        println!(
            "    playback path (rodio Player on a headless mixer): best lag {lag:+} samples \
             ({:+.2} ms)",
            lag as f64 / f64::from(RATE) * 1000.0
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let short = args.iter().any(|arg| arg == "--short");
    let (length_s, clicks) = if short { (4, 3) } else { (11, 10) };
    let reference = click_track(length_s, clicks);
    let mut files: Vec<PathBuf> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--short" {
            continue;
        }
        if arg == "--write" {
            if let Some(out) = iter.next() {
                match write_wav_mono16(Path::new(out), &reference) {
                    Ok(()) => println!("wrote {out}"),
                    Err(error) => println!("cannot write {out}: {error}"),
                }
            }
        } else {
            files.push(PathBuf::from(arg));
        }
    }
    for file in &files {
        report(file, &reference, length_s);
    }
}
