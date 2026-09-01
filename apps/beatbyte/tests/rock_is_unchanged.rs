//! The rock regression gate.
//!
//! The loop-house work changes how a beat grid is produced. The
//! commission's one hard constraint is that rock must not regress —
//! but "rock is good" has only ever been a judgement by ear, and
//! `docs/audio-eval-baseline.md` records that the built-in songs have
//! no annotated grids to measure against.
//!
//! So the gate is exactness instead of tolerance: with the shipped
//! configuration, the two built-in songs must generate **byte for
//! byte the same charts** they generate today. A metric with 2 %
//! slack would let a real behaviour change hide inside the slack;
//! a hash cannot. When a change to the default IS intended, this test
//! fails, and updating the constant is the deliberate act of
//! recording that decision.
//!
//! The hash is FNV-1a written out here on purpose: `DefaultHasher`
//! makes no stability promise across Rust releases, and CI installs
//! the latest stable, so a toolchain update would look like an
//! analysis regression.
//!
//! ## When the constants moved
//!
//! **v0.13.22** — the shipped grid became the tracked one. Both
//! songs' charts changed, deliberately, and the evidence is in
//! `docs/audio-eval-baseline.md`: measured against their exact
//! rendered grids, `circuit-breaker` went from a beat F-measure of
//! **0.000 to 0.982** (the 146 ms phase error that Phase 1 found is
//! gone) and `solder-groove` held at 0.995. No case with ground
//! truth anywhere in the repository got worse.

use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_chart::generate::{GenerateMeta, generate_chart};

/// FNV-1a over the chart's canonical JSON.
fn fingerprint(json: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in json.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn chart_fingerprint(audio: &beatbyte_audio::decode::AudioData, title: &str) -> u64 {
    let analysis = SpectralAnalyzer::default().analyze(audio);
    let chart = generate_chart(
        &analysis,
        &GenerateMeta {
            title: title.to_owned(),
            artist: "BeatByte".to_owned(),
            audio: "song.wav".to_owned(),
        },
    );
    let json = serde_json::to_string(&chart).expect("a chart serialises");
    fingerprint(&json)
}

#[test]
fn the_demo_song_generates_the_same_chart_it_always_has() {
    assert_eq!(
        chart_fingerprint(&beatbyte_audio::demo::render_demo_song(), "Circuit Breaker"),
        18_321_509_161_578_243_430,
        "the demo song's chart changed; if that was intended, update \
         the constant and say so in the CHANGELOG"
    );
}

#[test]
fn the_groove_song_generates_the_same_chart_it_always_has() {
    assert_eq!(
        chart_fingerprint(&beatbyte_audio::demo::render_groove_song(), "Solder Groove"),
        17_102_777_573_325_627_102,
        "the groove song's chart changed; if that was intended, update \
         the constant and say so in the CHANGELOG"
    );
}
