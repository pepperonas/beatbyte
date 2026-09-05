//! Which audio formats can actually be imported — verified, not
//! assumed.
//!
//! The fixtures are a half-second synthesized tone (440 Hz + 660 Hz
//! overtone, enveloped), rendered by `tests/fixtures/README.md`'s
//! recipe — fully original audio, a few KB each. Every claim the
//! README makes about importable formats must have a passing decode
//! here; every known-unsupported format is pinned too, so a decoder
//! upgrade that changes the truth fails a test instead of silently
//! outdating the docs.

use std::path::PathBuf;

use beatbyte_audio::decode_file;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Decode one fixture and sanity-check the result.
fn assert_decodes(name: &str) {
    let audio = decode_file(&fixture(name)).unwrap_or_else(|error| {
        panic!("`{name}` should decode, got: {error}");
    });
    // Half a second of tone; lossy encoders may pad the edges
    // (LAME's encoder delay alone is ~1100 samples), so bounds are
    // generous but still catch a wrong-rate or truncated decode.
    assert!(
        (0.35..0.9).contains(&audio.duration_s()),
        "`{name}`: expected ~0.5 s, got {:.3} s",
        audio.duration_s()
    );
    let peak = audio.samples().iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(
        peak > 0.05,
        "`{name}`: decoded audio is near-silent (peak {peak})"
    );
}

#[test]
fn wav_decodes() {
    assert_decodes("tone.wav");
}

#[test]
fn ogg_vorbis_decodes() {
    // Stereo on purpose — also exercises the stereo→mono downmix.
    assert_decodes("tone.ogg");
}

#[test]
fn flac_decodes() {
    assert_decodes("tone.flac");
}

#[test]
fn mp3_decodes() {
    assert_decodes("tone.mp3");
}

#[test]
fn m4a_aac_decodes() {
    // Written first as a "not supported" pin — it failed immediately
    // because the bundled decoder DOES read AAC/M4A. The docs were
    // updated to match; this keeps them honest.
    assert_decodes("tone.m4a");
}

// ---- Where the first sample lands ---------------------------------------
//
// A lossy container can carry *encoder delay*: AAC encoders emit
// priming samples (Apple: 2112, FFmpeg: 1024) and declare them in the
// container (`iTunSMPB`, resp. an `elst` edit list) so a player skips
// them. MP3 encoders declare theirs in the LAME/Xing header. Whether
// the decode honours those declarations decides whether the decoded
// timeline is the master's timeline — which is what every lyric file
// from outside was written against.
//
// Measured 2026-09-05 (`docs/audio/decode-offset.md`): Symphonia
// 0.5.5 applies the LAME delay but parses the MP4 edit list without
// using it, so `.m4a` decoded late by its priming — 23 ms for the
// FFmpeg encodes 70 of the 71 library files are. `beatbyte-audio`
// reads the declaration itself now (`priming::container_priming`)
// and both decode paths skip it. These pin that every container
// lands on the master's timeline; the audit example measures the
// playback path the same way.

/// The click fixtures: one full-scale sample at 1, 2 and 3 s in a
/// 4-second 44.1 kHz mono track (`examples/click_offset.rs --short`).
fn click_reference() -> Vec<f32> {
    let rate = 44_100usize;
    let mut samples = vec![0.0f32; rate * 4];
    for k in 1..=3 {
        samples[k * rate] = 1.0;
    }
    samples
}

/// The lag, in samples, at which the decode best matches the
/// reference (positive = late), searched over ±4000 samples. A lossy
/// codec smears a click; the cross-correlation peak survives that.
fn decode_lag(name: &str) -> i64 {
    let audio = decode_file(&fixture(name)).unwrap_or_else(|error| {
        panic!("`{name}` should decode, got: {error}");
    });
    assert_eq!(
        audio.sample_rate(),
        44_100,
        "`{name}` decoded at the wrong rate"
    );
    let reference = click_reference();
    let decoded = audio.samples();
    let mut best = (0i64, f64::MIN);
    for lag in -4000i64..=4000 {
        let mut acc = 0.0f64;
        for (i, &r) in reference.iter().enumerate() {
            if r == 0.0 {
                continue;
            }
            let j = i as i64 + lag;
            if j >= 0 && (j as usize) < decoded.len() {
                acc += f64::from(r) * f64::from(decoded[j as usize]);
            }
        }
        if acc > best.1 {
            best = (lag, acc);
        }
    }
    best.0
}

#[test]
fn mp3_decodes_on_the_masters_timeline() {
    // LAME writes its encoder delay into the header and the decoder
    // honours it (rodio's gapless setting is on by default).
    assert_eq!(decode_lag("click-lame.mp3"), 0);
}

#[test]
fn m4a_from_ffmpeg_decodes_on_the_masters_timeline() {
    // FFmpeg's AAC encoder: 1024 priming samples, declared in an
    // `elst` edit list the demuxer ignores — before the skip this
    // measured +1024 (23.2 ms).
    assert_eq!(decode_lag("click-ffmpeg.m4a"), 0);
}

#[test]
fn m4a_from_apple_decodes_on_the_masters_timeline() {
    // Apple's encoder: 2112 priming samples, declared in `iTunSMPB`
    // (and an edit list) — before the skip this measured +2112
    // (47.9 ms).
    assert_eq!(decode_lag("click-apple.m4a"), 0);
}

#[test]
fn the_decode_reports_what_it_skipped() {
    // The chart a decode feeds records this, so a chart made before
    // the skip can be told apart from one made after it.
    let apple = decode_file(&fixture("click-apple.m4a")).expect("decodes");
    assert_eq!(apple.priming().samples, 2112);
    let mp3 = decode_file(&fixture("click-lame.mp3")).expect("decodes");
    assert_eq!(mp3.priming().samples, 0);
}
