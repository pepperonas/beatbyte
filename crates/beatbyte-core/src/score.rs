//! Scoring rules and per-player performance tracking.
//!
//! Everything here is a pure state machine: feed it judgments, sustains
//! and Hype events, read out score/combo/accuracy. Multiplayer is N
//! independent [`PlayerPerformance`] values — there is no global scoring
//! state.

use serde::{Deserialize, Serialize};

use crate::timing::Judgment;

/// Data-driven scoring configuration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScoreConfig {
    /// Points for a Perfect hit (per lane in a chord).
    pub points_perfect: u32,
    /// Points for a Great hit (per lane in a chord).
    pub points_great: u32,
    /// Points for a Good hit (per lane in a chord).
    pub points_good: u32,
    /// Points per musical beat of sustain held (before multiplier).
    pub sustain_points_per_beat: f64,
    /// Streak length required per multiplier step (×2 at 1×this, ×3 at
    /// 2×this, …).
    pub streak_per_level: u32,
    /// Maximum streak multiplier (before Hype).
    pub max_multiplier: u32,
    /// Multiplier applied on top while Hype is active.
    pub hype_multiplier: u32,
    /// Meter gained per completed special phrase (meter range 0–1).
    pub hype_per_phrase: f64,
    /// Minimum meter required to activate Hype.
    pub hype_activation_threshold: f64,
    /// Beats of song time a full Hype meter lasts.
    pub hype_full_duration_beats: f64,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        ScoreConfig {
            points_perfect: 50,
            points_great: 35,
            points_good: 20,
            sustain_points_per_beat: 25.0,
            streak_per_level: 10,
            max_multiplier: 4,
            hype_multiplier: 2,
            hype_per_phrase: 0.25,
            hype_activation_threshold: 0.5,
            hype_full_duration_beats: 32.0,
        }
    }
}

impl ScoreConfig {
    /// Base points for a judgment (per lane in a chord).
    #[must_use]
    pub const fn points_for(&self, judgment: Judgment) -> u32 {
        match judgment {
            Judgment::Perfect => self.points_perfect,
            Judgment::Great => self.points_great,
            Judgment::Good => self.points_good,
            Judgment::Miss => 0,
        }
    }
}

/// Counts per judgment tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JudgmentCounts {
    /// Number of Perfect hits.
    pub perfect: u32,
    /// Number of Great hits.
    pub great: u32,
    /// Number of Good hits.
    pub good: u32,
    /// Number of missed note events.
    pub miss: u32,
}

impl JudgmentCounts {
    /// Total judged note events.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.perfect + self.great + self.good + self.miss
    }

    /// Total hit (non-miss) note events.
    #[must_use]
    pub const fn hits(&self) -> u32 {
        self.perfect + self.great + self.good
    }
}

/// One player's live performance state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerPerformance {
    config: ScoreConfig,
    score: u64,
    streak: u32,
    best_streak: u32,
    counts: JudgmentCounts,
    overstrums: u32,
    /// Fractional sustain points not yet banked into `score`.
    sustain_accum: f64,
    /// Hype meter, 0.0–1.0.
    hype_meter: f64,
    hype_active: bool,
}

impl PlayerPerformance {
    /// Start a fresh performance with the given scoring rules.
    #[must_use]
    pub fn new(config: ScoreConfig) -> PlayerPerformance {
        PlayerPerformance {
            config,
            score: 0,
            streak: 0,
            best_streak: 0,
            counts: JudgmentCounts::default(),
            overstrums: 0,
            sustain_accum: 0.0,
            hype_meter: 0.0,
            hype_active: false,
        }
    }

    /// The scoring configuration in use.
    #[must_use]
    pub fn config(&self) -> &ScoreConfig {
        &self.config
    }

    /// Current score.
    #[must_use]
    pub fn score(&self) -> u64 {
        self.score
    }

    /// Current streak (consecutive hits without miss/overstrum).
    #[must_use]
    pub fn streak(&self) -> u32 {
        self.streak
    }

    /// Best streak so far.
    #[must_use]
    pub fn best_streak(&self) -> u32 {
        self.best_streak
    }

    /// Judgment counts so far.
    #[must_use]
    pub fn counts(&self) -> JudgmentCounts {
        self.counts
    }

    /// Number of overstrums (strums that matched no note).
    #[must_use]
    pub fn overstrums(&self) -> u32 {
        self.overstrums
    }

    /// Hype meter level, 0.0–1.0.
    #[must_use]
    pub fn hype_meter(&self) -> f64 {
        self.hype_meter
    }

    /// Whether Hype is currently active.
    #[must_use]
    pub fn hype_active(&self) -> bool {
        self.hype_active
    }

    /// The streak multiplier (without Hype): ×1 up to ×`max_multiplier`.
    #[must_use]
    pub fn streak_multiplier(&self) -> u32 {
        let level = 1 + self.streak / self.config.streak_per_level.max(1);
        level.min(self.config.max_multiplier)
    }

    /// The full multiplier including Hype.
    #[must_use]
    pub fn multiplier(&self) -> u32 {
        let base = self.streak_multiplier();
        if self.hype_active {
            base * self.config.hype_multiplier
        } else {
            base
        }
    }

    /// Register a judged note event. `lane_count` is the chord size
    /// (1 for single notes); chord hits score per lane.
    pub fn register_judgment(&mut self, judgment: Judgment, lane_count: usize) {
        match judgment {
            Judgment::Perfect => self.counts.perfect += 1,
            Judgment::Great => self.counts.great += 1,
            Judgment::Good => self.counts.good += 1,
            Judgment::Miss => {
                self.counts.miss += 1;
                self.streak = 0;
                return;
            }
        }
        self.streak += 1;
        self.best_streak = self.best_streak.max(self.streak);
        // Multiplier is applied *after* the streak update, so the note
        // that reaches a threshold already benefits from it.
        let base = u64::from(self.config.points_for(judgment)) * lane_count as u64;
        self.score += base * u64::from(self.multiplier());
    }

    /// Register a strum that matched no note (overstrum): breaks the
    /// streak but scores no miss (the note count is untouched).
    pub fn register_overstrum(&mut self) {
        self.overstrums += 1;
        self.streak = 0;
    }

    /// Award sustain hold time, measured in musical beats.
    pub fn add_sustain_beats(&mut self, beats: f64) {
        if beats <= 0.0 || !beats.is_finite() {
            return;
        }
        self.sustain_accum +=
            beats * self.config.sustain_points_per_beat * f64::from(self.multiplier());
        // Bank whole points, keep the fraction accumulating.
        let whole = self.sustain_accum.floor();
        if whole > 0.0 {
            self.score += whole as u64;
            self.sustain_accum -= whole;
        }
    }

    /// Award Hype meter for a completed special phrase.
    pub fn complete_phrase(&mut self) {
        self.hype_meter = (self.hype_meter + self.config.hype_per_phrase).min(1.0);
    }

    /// Try to activate Hype. Returns whether activation happened.
    pub fn try_activate_hype(&mut self) -> bool {
        if !self.hype_active && self.hype_meter >= self.config.hype_activation_threshold {
            self.hype_active = true;
            true
        } else {
            false
        }
    }

    /// Drain the active Hype meter by elapsed musical beats; deactivates
    /// when the meter empties.
    pub fn drain_hype_beats(&mut self, beats: f64) {
        if !self.hype_active || beats <= 0.0 || !beats.is_finite() {
            return;
        }
        let drain = beats / self.config.hype_full_duration_beats.max(f64::EPSILON);
        self.hype_meter = (self.hype_meter - drain).max(0.0);
        if self.hype_meter <= 0.0 {
            self.hype_active = false;
        }
    }

    /// Weighted accuracy in 0.0–1.0 (Perfect = 1.0; see
    /// [`Judgment::accuracy_weight`]). Returns 1.0 before any judgment.
    #[must_use]
    pub fn accuracy(&self) -> f64 {
        let total = self.counts.total();
        if total == 0 {
            return 1.0;
        }
        let weighted = f64::from(self.counts.perfect) * Judgment::Perfect.accuracy_weight()
            + f64::from(self.counts.great) * Judgment::Great.accuracy_weight()
            + f64::from(self.counts.good) * Judgment::Good.accuracy_weight();
        weighted / f64::from(total)
    }

    /// Fraction of note events hit (any tier), 0.0–1.0.
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.counts.total();
        if total == 0 {
            return 1.0;
        }
        f64::from(self.counts.hits()) / f64::from(total)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn perf() -> PlayerPerformance {
        PlayerPerformance::new(ScoreConfig::default())
    }

    #[test]
    fn perfect_hit_scores_base_points() {
        let mut p = perf();
        p.register_judgment(Judgment::Perfect, 1);
        assert_eq!(p.score(), 50);
        assert_eq!(p.streak(), 1);
    }

    #[test]
    fn chords_score_per_lane() {
        let mut p = perf();
        p.register_judgment(Judgment::Perfect, 3);
        assert_eq!(p.score(), 150);
    }

    #[test]
    fn judgment_tiers_score_differently() {
        let mut p = perf();
        p.register_judgment(Judgment::Great, 1);
        p.register_judgment(Judgment::Good, 1);
        assert_eq!(p.score(), 35 + 20);
    }

    #[test]
    fn miss_resets_streak_and_scores_nothing() {
        let mut p = perf();
        for _ in 0..5 {
            p.register_judgment(Judgment::Perfect, 1);
        }
        let score_before = p.score();
        p.register_judgment(Judgment::Miss, 1);
        assert_eq!(p.score(), score_before);
        assert_eq!(p.streak(), 0);
        assert_eq!(p.counts().miss, 1);
        assert_eq!(p.best_streak(), 5);
    }

    #[test]
    fn multiplier_steps_at_streak_thresholds() {
        let mut p = perf();
        assert_eq!(p.multiplier(), 1);
        for _ in 0..9 {
            p.register_judgment(Judgment::Perfect, 1);
        }
        assert_eq!(p.streak(), 9);
        assert_eq!(p.multiplier(), 1);
        p.register_judgment(Judgment::Perfect, 1); // 10th note: reaches ×2
        assert_eq!(p.multiplier(), 2);
        for _ in 0..10 {
            p.register_judgment(Judgment::Perfect, 1);
        }
        assert_eq!(p.multiplier(), 3);
        for _ in 0..10 {
            p.register_judgment(Judgment::Perfect, 1);
        }
        assert_eq!(p.multiplier(), 4);
        // Caps at max.
        for _ in 0..50 {
            p.register_judgment(Judgment::Perfect, 1);
        }
        assert_eq!(p.multiplier(), 4);
    }

    #[test]
    fn threshold_note_already_gets_the_new_multiplier() {
        let mut p = perf();
        for _ in 0..9 {
            p.register_judgment(Judgment::Perfect, 1);
        }
        let before = p.score();
        p.register_judgment(Judgment::Perfect, 1);
        assert_eq!(p.score() - before, 100, "10th note scores at ×2");
    }

    #[test]
    fn overstrum_breaks_streak_without_counting_a_note() {
        let mut p = perf();
        p.register_judgment(Judgment::Perfect, 1);
        p.register_overstrum();
        assert_eq!(p.streak(), 0);
        assert_eq!(p.overstrums(), 1);
        assert_eq!(p.counts().total(), 1);
    }

    #[test]
    fn sustain_points_accumulate_with_multiplier() {
        let mut p = perf();
        // One beat of sustain at ×1 = 25 points.
        p.add_sustain_beats(1.0);
        assert_eq!(p.score(), 25);
        // Fractions accumulate without loss: 4 × 0.25 beats = 25 points.
        for _ in 0..4 {
            p.add_sustain_beats(0.25);
        }
        assert_eq!(p.score(), 50);
    }

    #[test]
    fn hype_doubles_the_full_multiplier() {
        let mut p = perf();
        let per_level = p.config().streak_per_level;
        for _ in 0..per_level {
            p.register_judgment(Judgment::Perfect, 1);
        }
        let base = p.multiplier();
        assert!(base > 1, "a full streak level must raise the multiplier");
        p.complete_phrase();
        p.complete_phrase();
        assert!(p.try_activate_hype());
        assert_eq!(
            p.multiplier(),
            base * p.config().hype_multiplier,
            "hype multiplies the streak multiplier, not replaces it"
        );
    }

    #[test]
    fn untouched_performance_reads_as_perfect() {
        // Accuracy over zero notes is 1.0 by definition — a fresh
        // results screen must never show 0% before the first note.
        let p = perf();
        assert!((p.accuracy() - 1.0).abs() < f64::EPSILON);
        assert_eq!(p.score(), 0);
        assert_eq!(p.streak(), 0);
    }

    #[test]
    fn hit_rate_counts_all_tiers_but_not_misses() {
        let mut p = perf();
        p.register_judgment(Judgment::Perfect, 1);
        p.register_judgment(Judgment::Great, 1);
        p.register_judgment(Judgment::Good, 1);
        p.register_judgment(Judgment::Miss, 1);
        assert!((p.hit_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn hype_lifecycle() {
        let mut p = perf();
        assert!(!p.try_activate_hype(), "empty meter cannot activate");

        p.complete_phrase();
        assert!((p.hype_meter() - 0.25).abs() < 1e-9);
        assert!(!p.try_activate_hype(), "below activation threshold");

        p.complete_phrase();
        assert!(p.try_activate_hype());
        assert!(p.hype_active());
        assert_eq!(p.multiplier(), 2, "hype doubles the ×1 multiplier");

        // Full meter lasts 32 beats; half meter drains in 16.
        p.drain_hype_beats(16.0);
        assert!(!p.hype_active(), "meter empty → hype ends");
        assert_eq!(p.multiplier(), 1);
    }

    #[test]
    fn hype_meter_caps_at_full() {
        let mut p = perf();
        for _ in 0..10 {
            p.complete_phrase();
        }
        assert!((p.hype_meter() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn accuracy_is_weighted() {
        let mut p = perf();
        assert!((p.accuracy() - 1.0).abs() < 1e-9, "no notes yet = 100%");
        p.register_judgment(Judgment::Perfect, 1);
        p.register_judgment(Judgment::Miss, 1);
        assert!((p.accuracy() - 0.5).abs() < 1e-9);
        assert!((p.hit_rate() - 0.5).abs() < 1e-9);
    }
}
