//! Pairing a local corpus of audio with the grids that describe it.
//!
//! The real corpus lives outside the repository and stays there: no
//! audio, no grid and no library file is checked in. What this module
//! provides is the small amount of machinery needed to walk two local
//! directories — one of Rekordbox analysis files, one of audio — and
//! line them up by file name, which is the only identifier the
//! analysis file carries once its volume marker is stripped.
//!
//! The selection rules are pure and tested; only the walk touches the
//! filesystem.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::eval::{GroundTruth, anlz};

/// One track with the grid that describes it.
pub struct Paired {
    /// The audio file's name, as both sides know it.
    pub name: String,
    /// Where the audio is on this machine.
    pub audio: PathBuf,
    /// The grid Rekordbox recorded for it.
    pub truth: GroundTruth,
}

/// Which tracks a run is interested in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    /// Inclusive tempo range.
    pub bpm: (f64, f64),
    /// Shortest grid worth measuring drift on, in seconds.
    pub min_len_s: f64,
}

impl Profile {
    /// The commission's target material: loop house in DJ format.
    #[must_use]
    pub const fn loop_house() -> Self {
        Self {
            bpm: (118.0, 130.0),
            min_len_s: 300.0,
        }
    }

    /// Does this grid belong to the profile? Pure — tested.
    ///
    /// The length is taken from the grid rather than the audio on
    /// purpose: a grid that stops after 30 s describes 30 s of track,
    /// whatever the file's duration claims.
    #[must_use]
    pub fn admits(&self, truth: &GroundTruth) -> bool {
        let last = truth.beats.last().copied().unwrap_or(0.0);
        truth.bpm >= self.bpm.0 && truth.bpm <= self.bpm.1 && last >= self.min_len_s
    }
}

/// Every file under `root`, depth first.
///
/// Bounded by the tree itself — symlinked cycles are not followed
/// because only real directories are descended into. Unreadable
/// directories are skipped rather than fatal: a corpus is a working
/// directory, not a curated archive.
#[must_use]
pub fn walk(root: &Path) -> Vec<PathBuf> {
    let (mut files, mut stack) = (Vec::new(), vec![root.to_path_buf()]);
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// Index paths by file name, first one wins.
///
/// First-wins rather than last-wins so a run is reproducible when the
/// same track sits in two folders: the walk order is stable, so the
/// choice is too. Pure — tested.
#[must_use]
pub fn index_by_name(files: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    for path in files {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            index.entry(name.to_owned()).or_insert_with(|| path.clone());
        }
    }
    index
}

/// Walk both roots and return the tracks in `profile`, sorted by
/// tempo so a report table reads in a stable order.
///
/// A grid without its audio is silently skipped — a library holds
/// analysis files for tracks that have since been moved, and that is
/// normal rather than an error worth stopping for.
#[must_use]
pub fn pair(anlz_root: &Path, audio_root: &Path, profile: Profile) -> Vec<Paired> {
    let audio = index_by_name(&walk(audio_root));
    let mut found = Vec::new();
    for file in walk(anlz_root) {
        if file.file_name().and_then(|n| n.to_str()) != Some("ANLZ0000.DAT") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let Some(track) = anlz::parse(&bytes) else {
            continue;
        };
        if !profile.admits(&track.truth) {
            continue;
        }
        let name = anlz::file_name(&track.path).to_owned();
        let Some(path) = audio.get(&name) else {
            continue;
        };
        found.push(Paired {
            name,
            audio: path.clone(),
            truth: track.truth,
        });
    }
    found.sort_by(|a, b| a.truth.bpm.total_cmp(&b.truth.bpm));
    found
}

/// The signed distance from `t` to the nearest reference beat.
///
/// ⚠️ This wraps at half a period: a grid 232 ms early and one 191 ms
/// late are 65 ms apart, not 423. Two of these must never be
/// subtracted to obtain a drift — derive drift from the tempo error,
/// which has no wrap. Pure — tested.
#[must_use]
pub fn residual(t: f64, refs: &[f64]) -> Option<f64> {
    refs.iter()
        .map(|r| t - r)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// How far a grid at `estimated` BPM slides against one at `truth`
/// over `duration_s`, in seconds.
///
/// The wrap-free companion to [`residual`]: a relative tempo error
/// accumulates linearly, so this is simply how much grid time the
/// error has bought by the end. Pure — tested.
#[must_use]
pub fn accumulated_drift_s(estimated: f64, truth: f64, duration_s: f64) -> f64 {
    if truth <= 0.0 {
        return 0.0;
    }
    ((estimated - truth) / truth).abs() * duration_s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(bpm: f64, beats: usize) -> GroundTruth {
        GroundTruth::steady(bpm, 0.0, beats)
    }

    #[test]
    fn the_profile_admits_only_long_tracks_in_range() {
        let profile = Profile::loop_house();
        // 125 BPM, 800 beats = 384 s.
        assert!(profile.admits(&grid(125.0, 800)));
        // Right tempo, far too short to say anything about drift.
        assert!(!profile.admits(&grid(125.0, 60)));
        // Long enough, wrong material.
        assert!(!profile.admits(&grid(174.0, 1200)));
        assert!(!profile.admits(&grid(90.0, 1200)));
    }

    #[test]
    fn the_profile_measures_length_by_the_grid_not_the_count() {
        // 800 beats at 240 BPM is only 200 s — a beat count alone
        // would wrongly admit it.
        let profile = Profile {
            bpm: (200.0, 260.0),
            min_len_s: 300.0,
        };
        assert!(!profile.admits(&grid(240.0, 800)));
        assert!(profile.admits(&grid(240.0, 1400)));
    }

    #[test]
    fn indexing_keeps_the_first_of_a_duplicated_name() {
        let files = [
            PathBuf::from("/a/Track.mp3"),
            PathBuf::from("/b/Track.mp3"),
            PathBuf::from("/b/Other.mp3"),
        ];
        let index = index_by_name(&files);
        assert_eq!(index.len(), 2);
        assert_eq!(index.get("Track.mp3"), Some(&PathBuf::from("/a/Track.mp3")));
    }

    #[test]
    fn the_residual_takes_the_nearest_beat_on_either_side() {
        let refs = [0.0, 0.5, 1.0];
        // Just after a beat: a small positive residual.
        let after = residual(0.52, &refs).expect("a residual");
        assert!((after - 0.02).abs() < 1e-9);
        // Just before one: a small NEGATIVE residual, not a large
        // positive one against the previous beat.
        let before = residual(0.48, &refs).expect("a residual");
        assert!((before + 0.02).abs() < 1e-9);
        assert!(residual(0.4, &[]).is_none());
    }

    #[test]
    fn the_residual_wraps_at_half_a_period() {
        // THE reason drift is not computed from two residuals. A grid
        // 0.24 s late against a 0.5 s period reads as 0.24; one more
        // millisecond of lateness reads as -0.259, a sign flip and an
        // apparent half-second jump.
        let refs = [0.0, 0.5, 1.0];
        let late = residual(0.24, &refs).expect("a residual");
        let later = residual(0.26, &refs).expect("a residual");
        assert!(late > 0.0 && later < 0.0, "the sign flips at half");
        assert!(
            (later - late).abs() > 0.4,
            "and the apparent jump is nearly a whole period, \
             which is why two residuals must never be subtracted"
        );
    }

    #[test]
    fn drift_is_the_tempo_error_times_the_length() {
        // The BICEP case from the baseline: -0.250 % over 496 s.
        let drift = accumulated_drift_s(126.68, 127.0, 496.0);
        assert!(
            (drift - 1.249).abs() < 0.01,
            "expected ~1.25 s of slide, got {drift}"
        );
        // Direction does not matter — a grid is equally wrong either
        // way round.
        assert!(
            (accumulated_drift_s(128.0, 127.0, 100.0) - accumulated_drift_s(126.0, 127.0, 100.0))
                .abs()
                < 1e-9
        );
        // A nonsense reference cannot produce a nonsense answer.
        assert!((accumulated_drift_s(120.0, 0.0, 100.0)).abs() < f64::EPSILON);
    }
}
