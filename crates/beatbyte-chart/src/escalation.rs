//! Energy-aware escalation — the design pattern that won its by-ear
//! A/B twice (first on one song, then across the library) graduating
//! into the generator itself (ADR-0011's payoff: better generation
//! defaults from recorded verdicts).
//!
//! The pattern: **escalate where the song escalates.** A difficulty's
//! density rises toward the next difficulty's reading in the song's
//! own high-energy passages (refrains), and stays at its anchor
//! everywhere else — verses keep breathing, pauses stay gameplay.
//! Uniform densification is exactly what this replaces.
//!
//! The hot passages are found the way the winning design sessions
//! found them: from the song's OWN energy percentiles (p70, stepping
//! to p80/p90 when that floods), single-bar holes smoothed, runs
//! shorter than four bars dropped — an escalation needs room to read.

use beatbyte_core::SongAnalysis;

/// Fraction of the song that may be hot before the percentile ladder
/// steps up: escalation that covers most of the song is not
/// escalation, it is a louder uniform density.
const MAX_HOT_SHARE: f64 = 0.55;

/// A run of hot bars must be at least this long to survive.
const MIN_RUN_BARS: usize = 4;

/// The percentile ladder, tried in order until the result is
/// selective.
const LADDER: [f64; 3] = [0.70, 0.80, 0.90];

/// Which bars of the song are its own high ground.
///
/// Returns one flag per bar (4 beats, from `grid_origin_s`). All
/// pure arithmetic over the analysis — same analysis, same flags,
/// every run (determinism is a hard rule).
#[must_use]
pub fn hot_bar_flags(analysis: &SongAnalysis, grid_origin_s: f64) -> Vec<bool> {
    let beat = analysis.beat_interval_s();
    if beat <= 0.0 || analysis.energy.is_empty() || analysis.energy_hop_s <= 0.0 {
        return Vec::new();
    }
    let bar_s = beat * 4.0;
    let bar_count = (((analysis.duration_s - grid_origin_s) / bar_s).ceil()).max(0.0) as usize;
    if bar_count == 0 {
        return Vec::new();
    }

    // Mean energy per bar.
    let energies: Vec<f32> = (0..bar_count)
        .map(|bar| {
            let start = (bar as f64).mul_add(bar_s, grid_origin_s);
            let end = start + bar_s;
            let from = ((start / analysis.energy_hop_s).floor().max(0.0)) as usize;
            let to = ((end / analysis.energy_hop_s).ceil().max(0.0)) as usize;
            let window =
                &analysis.energy[from.min(analysis.energy.len())..to.min(analysis.energy.len())];
            if window.is_empty() {
                0.0
            } else {
                window.iter().sum::<f32>() / window.len() as f32
            }
        })
        .collect();

    let mut sorted = energies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile =
        |p: f64| -> f32 { sorted[((sorted.len() as f64 * p) as usize).min(sorted.len() - 1)] };

    for p in LADDER {
        let threshold = percentile(p);
        let flags = shape(&energies, threshold);
        let hot = flags.iter().filter(|f| **f).count();
        if hot > 0 && (hot as f64) <= MAX_HOT_SHARE * bar_count as f64 {
            return flags;
        }
    }
    // Nothing selective at any rung: a song with no high ground of
    // its own gets no escalation. "Nothing to escalate" is a finding,
    // not a failure.
    vec![false; bar_count]
}

/// Threshold → smoothed, de-islanded flags.
fn shape(energies: &[f32], threshold: f32) -> Vec<bool> {
    let mut flags: Vec<bool> = energies.iter().map(|e| *e >= threshold).collect();
    // A one-bar dip inside a refrain must not flip the feel back and
    // forth.
    for i in 1..flags.len().saturating_sub(1) {
        if !flags[i] && flags[i - 1] && flags[i + 1] {
            flags[i] = true;
        }
    }
    // Isolated islands cannot be read as escalation.
    let mut i = 0;
    while i < flags.len() {
        if flags[i] {
            let start = i;
            while i < flags.len() && flags[i] {
                i += 1;
            }
            if i - start < MIN_RUN_BARS {
                for flag in &mut flags[start..i] {
                    *flag = false;
                }
            }
        } else {
            i += 1;
        }
    }
    flags
}

/// Which bar a song time falls into (clamped at zero for pre-roll
/// times).
#[must_use]
pub fn bar_of(time_s: f64, grid_origin_s: f64, beat_s: f64) -> usize {
    if beat_s <= 0.0 || time_s <= grid_origin_s {
        return 0;
    }
    ((time_s - grid_origin_s) / (beat_s * 4.0)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::SongAnalysis;

    /// 120 BPM (bar = 2 s), energy per second so bars map 2:1.
    fn analysis(energy: Vec<f32>, duration_s: f64) -> SongAnalysis {
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 1.0,
            alt_bpm: None,
            beats: (0..(duration_s * 2.0) as usize)
                .map(|i| f64::from(i as u32) * 0.5)
                .collect(),
            onsets: Vec::new(),
            energy,
            energy_hop_s: 1.0,
            duration_s,
            melody: Vec::new(),
        }
    }

    /// 32 bars: quiet floor with one 8-bar refrain in the middle.
    fn one_refrain() -> SongAnalysis {
        let mut energy = vec![0.3f32; 64]; // 64 s = 32 bars
        for e in &mut energy[24..40] {
            *e = 0.9; // bars 12..20
        }
        analysis(energy, 64.0)
    }

    #[test]
    fn the_refrain_is_found_and_nothing_else() {
        let flags = hot_bar_flags(&one_refrain(), 0.0);
        assert_eq!(flags.len(), 32);
        let hot: Vec<usize> = flags
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.then_some(i))
            .collect();
        assert_eq!(hot, (12..20).collect::<Vec<_>>(), "exactly the refrain");
    }

    #[test]
    fn a_flat_song_has_no_high_ground() {
        // Every rung of the ladder floods on a flat song; the honest
        // answer is "nothing to escalate", not "everything is".
        let flat = analysis(vec![0.5f32; 64], 64.0);
        let flags = hot_bar_flags(&flat, 0.0);
        assert!(
            flags.iter().all(|f| !f),
            "a flat song must not escalate anywhere"
        );
    }

    #[test]
    fn a_one_bar_dip_inside_a_refrain_is_smoothed_over() {
        let mut song = one_refrain();
        song.energy[30] = 0.1; // one quiet bar (bar 15) inside the refrain
        song.energy[31] = 0.1;
        let flags = hot_bar_flags(&song, 0.0);
        assert!(flags[15], "a one-bar dip must not flip the feel");
    }

    #[test]
    fn short_islands_do_not_escalate() {
        let mut energy = vec![0.3f32; 64];
        for e in &mut energy[10..14] {
            *e = 0.9; // bars 5..7 — a two-bar island
        }
        let flags = hot_bar_flags(&analysis(energy, 64.0), 0.0);
        assert!(
            flags.iter().all(|f| !f),
            "a two-bar island cannot be read as escalation"
        );
    }

    #[test]
    fn the_ladder_steps_up_until_selective() {
        // Two thirds of the song hot at p70: the ladder must step to
        // a higher rung and keep the escalation selective.
        let mut energy = vec![0.3f32; 64];
        for e in &mut energy[8..56] {
            *e = 0.8; // bars 4..28 = 75 % of the song
        }
        for e in &mut energy[24..40] {
            *e = 0.95; // bars 12..20 stand above even that
        }
        let flags = hot_bar_flags(&analysis(energy, 64.0), 0.0);
        let hot = flags.iter().filter(|f| **f).count();
        assert!(hot > 0, "the true peak must still be found");
        assert!(
            (hot as f64) <= MAX_HOT_SHARE * flags.len() as f64,
            "{hot} of {} bars hot - not selective",
            flags.len()
        );
    }

    #[test]
    fn flags_are_deterministic() {
        let a = hot_bar_flags(&one_refrain(), 0.0);
        let b = hot_bar_flags(&one_refrain(), 0.0);
        assert_eq!(a, b);
    }

    #[test]
    fn bar_of_maps_times_to_bars() {
        assert_eq!(bar_of(0.0, 0.0, 0.5), 0);
        assert_eq!(bar_of(1.9, 0.0, 0.5), 0);
        assert_eq!(bar_of(2.1, 0.0, 0.5), 1);
        // The grid origin shifts the grid, not the notes.
        assert_eq!(bar_of(2.1, 1.0, 0.5), 0);
        // Pre-roll times clamp instead of going negative.
        assert_eq!(bar_of(-0.5, 0.0, 0.5), 0);
    }
}
