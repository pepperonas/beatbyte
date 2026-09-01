//! The rock regression gate.
//!
//! The loop-house work changes how a beat grid is produced. The
//! commission's one hard constraint is that rock must not regress —
//! but "rock is good" has only ever been a judgement by ear, and
//! `docs/audio-eval-baseline.md` records that the built-in songs have
//! no annotated grids to measure against.
//!
//! So the gate is a fingerprint instead of a tolerance: with the
//! shipped configuration, the two built-in songs must generate the
//! same charts they generate today. A metric with 2 % slack would let
//! a real behaviour change hide inside the slack; a fingerprint
//! cannot. When a change to the default IS intended, this test fails,
//! and updating the constant is the deliberate act of recording that
//! decision.
//!
//! ## Why it fingerprints a projection and not the JSON
//!
//! The first version hashed the serialised chart, like
//! [`beatbyte_chart::schema::chart_hash`] does for provenance. It
//! passed on macOS and **failed on Linux in CI**, which is a real
//! finding rather than a flaky test: chart generation runs through
//! `ln`, `exp` and trigonometry, and platform libm implementations
//! differ in the last bits. Full-precision `f64` note times therefore
//! serialise differently on different machines.
//!
//! The fix is to fingerprint what a *player* could tell apart: note
//! counts, lanes, flags, and times rounded to whole milliseconds.
//! A millisecond is far below the tightest judgment window and far
//! above libm noise, so this catches every behaviour change worth
//! catching and none of the arithmetic weather. The residual risk is
//! named rather than denied: a note sitting exactly on a half-
//! millisecond boundary could still round two ways. Quantised times
//! are grid multiples, so that is unlikely rather than impossible,
//! and a single reproducible cross-platform mismatch here is
//! information, not noise to paper over. `chart_hash` keeps
//! hashing bytes, correctly — it binds a recorded session to the
//! exact file that was played, on the machine that played it.
//!
//! FNV-1a is written out here on purpose: `DefaultHasher` makes no
//! stability promise across Rust releases, and CI installs the latest
//! stable, so a toolchain update would look like an analysis
//! regression.
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
use beatbyte_chart::schema::ChartFile;

/// FNV-1a.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Everything about a chart a player could notice, at millisecond
/// resolution and in a fixed order.
fn projection(chart: &ChartFile) -> String {
    let mut out = String::new();
    // Millisecond rounding, so libm's last bits cannot move the
    // fingerprint but a real retiming certainly does.
    let ms = |seconds: f64| (seconds * 1000.0).round() as i64;
    out.push_str(&format!(
        "bpm={} offset={}\n",
        ms(chart.song.bpm),
        ms(chart.song.offset_s)
    ));
    for def in &chart.charts {
        out.push_str(&format!(
            "{:?} lanes={} notes={}\n",
            def.difficulty,
            def.lanes,
            def.notes.len()
        ));
        for note in &def.notes {
            out.push_str(&format!(
                "{} {} {} {}\n",
                ms(note.time),
                note.lane,
                ms(note.len),
                u8::from(note.hopo)
            ));
        }
        // Hype phrases are part of what gets played, so a phrase that
        // moved is a change even if every note stayed put.
        for phrase in &def.phrases {
            out.push_str(&format!("p {} {}\n", ms(phrase.start), ms(phrase.end)));
        }
    }
    out
}

fn fingerprint(audio: &beatbyte_audio::decode::AudioData, title: &str) -> u64 {
    let analysis = SpectralAnalyzer::default().analyze(audio);
    let chart = generate_chart(
        &analysis,
        &GenerateMeta {
            title: title.to_owned(),
            artist: "BeatByte".to_owned(),
            audio: "song.wav".to_owned(),
        },
    );
    fnv1a(projection(&chart).as_bytes())
}

#[test]
fn the_demo_song_generates_the_same_chart_it_always_has() {
    assert_eq!(
        fingerprint(&beatbyte_audio::demo::render_demo_song(), "Circuit Breaker"),
        9_619_993_056_299_140_922,
        "the demo song's chart changed; if that was intended, update \
         the constant and say so in the CHANGELOG"
    );
}

#[test]
fn the_groove_song_generates_the_same_chart_it_always_has() {
    assert_eq!(
        fingerprint(&beatbyte_audio::demo::render_groove_song(), "Solder Groove"),
        8_006_722_771_110_525_229,
        "the groove song's chart changed; if that was intended, update \
         the constant and say so in the CHANGELOG"
    );
}

/// The projection has to be blind to arithmetic noise and sharp about
/// real change, and neither half is obvious enough to assume.
#[test]
fn the_projection_ignores_sub_millisecond_noise_but_not_real_retiming() {
    let analysis = SpectralAnalyzer::default().analyze(&beatbyte_audio::demo::render_demo_song());
    let meta = GenerateMeta {
        title: "T".to_owned(),
        artist: "A".to_owned(),
        audio: "a.wav".to_owned(),
    };
    let chart = generate_chart(&analysis, &meta);
    let baseline = fnv1a(projection(&chart).as_bytes());

    // A libm-scale wobble: nanoseconds on every note.
    let mut jittered = chart.clone();
    for (index, def) in jittered.charts.iter_mut().enumerate() {
        for note in &mut def.notes {
            note.time += if index % 2 == 0 { 1e-9 } else { -1e-9 };
        }
    }
    assert_eq!(
        fnv1a(projection(&jittered).as_bytes()),
        baseline,
        "a nanosecond of float noise must not move the fingerprint, \
         or this test fails on whichever platform CI happens to use"
    );

    // A real retiming: one note, one millisecond.
    let mut moved = chart.clone();
    let note = moved
        .charts
        .iter_mut()
        .find_map(|def| def.notes.first_mut())
        .expect("the demo chart has notes");
    note.time += 0.001;
    assert_ne!(
        fnv1a(projection(&moved).as_bytes()),
        baseline,
        "a millisecond IS a change and must move the fingerprint"
    );

    // And a dropped note, which no rounding may hide.
    let mut thinner = chart.clone();
    thinner.charts[0].notes.pop();
    assert_ne!(fnv1a(projection(&thinner).as_bytes()), baseline);
}
