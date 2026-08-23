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
