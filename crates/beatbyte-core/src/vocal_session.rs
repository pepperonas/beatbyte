//! Judging a singer against a vocal chart.
//!
//! The counterpart of [`crate::session`] for the microphone: frames
//! of detected pitch in, note and phrase outcomes out, deterministic
//! and engine-free. Same inputs, same score, every time.
//!
//! ## What is judged, and what deliberately is not
//!
//! Four things, in the plan's weighting: **pitch** (65 %), **timing**
//! (15 %), **hold** (15 %) and **stability** (5 %). Pitch dominates
//! because it is the thing being asked for; the rest keep a note that
//! was merely touched from scoring like one that was sung.
//!
//! **Loudness is not one of them.** It decides whether a frame counts
//! at all — silence is not singing, and a clipped frame is not
//! trustworthy — and nothing else. Louder is never better: a game
//! that rewards volume teaches people to shout.
//!
//! **Rap and speech are not judged on pitch.** They have no stable
//! fundamental to be wrong about, so pitch and stability leave their
//! denominator entirely rather than scoring zero. A rapped line
//! scored as a failed sung one would punish the singer for the music.
//!
//! ## What it does not own
//!
//! Hype, the rock meter and the streak multiplier that the guitar
//! side already implements. This produces outcomes; the game feeds
//! them to the one implementation that already exists. A second Hype
//! would be two Hypes.

use serde::{Deserialize, Serialize};

use crate::Difficulty;
use crate::vocal::{PitchMode, VocalKind, VocalNote, VocalPart, VocalRange};

/// One estimate of what the microphone heard, on the song timeline.
///
/// The realtime engine produces a richer frame — capture stamps,
/// processing latency, ring-buffer state — and hands the part that
/// scoring needs to this. Diagnostics belong to the engine; judgment
/// reads only what it judges on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VocalInputFrame {
    /// When this was sung, in song seconds. Never a capture clock:
    /// the frame is mapped onto `song_time` before it gets here.
    pub song_time_s: f64,
    /// The pitch, fractional MIDI. `None` when unvoiced.
    pub midi: Option<f32>,
    /// The detector's confidence, `0..=1`.
    pub confidence: f32,
    /// The window's level. Used for gating, never for reward.
    pub rms_dbfs: f32,
    /// Whether the detector called this voiced.
    pub voiced: bool,
    /// Whether the input hit the rails.
    pub clipped: bool,
}

/// How a sung note is graded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocalGrade {
    /// Nothing usable arrived.
    Miss,
    /// Recognisably the note, and not much more.
    Weak,
    /// Sung.
    Good,
    /// Well sung.
    Great,
    /// On it.
    Perfect,
}

impl VocalGrade {
    /// The lower of two grades. A note perfectly in tune for a tenth
    /// of its length is not a Perfect, so the pitch grade and the
    /// grade its score earns are combined this way round.
    #[must_use]
    pub fn worse_of(self, other: VocalGrade) -> VocalGrade {
        self.min(other)
    }

    /// Whether the note counts as hit.
    #[must_use]
    pub fn hit(self) -> bool {
        self != VocalGrade::Miss
    }

    /// Its name, for the HUD and the results screen.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            VocalGrade::Perfect => "PERFECT",
            VocalGrade::Great => "GREAT",
            VocalGrade::Good => "GOOD",
            VocalGrade::Weak => "WEAK",
            VocalGrade::Miss => "MISS",
        }
    }
}

/// The numbers judgment runs on — one place, so a tolerance is tuned
/// rather than hunted for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VocalScoreConfig {
    /// Whether the written octave is the target.
    pub mode: PitchMode,
    /// At or under this many cents, a note is Perfect.
    pub perfect_cents: f32,
    /// … Great.
    pub great_cents: f32,
    /// … Good.
    pub good_cents: f32,
    /// … Weak. Beyond it, or unsung, a Miss.
    pub weak_cents: f32,
    /// How late or early a note's onset may be before timing scores
    /// zero.
    pub timing_window_s: f32,
    /// The spread of pitch error, in cents, at which stability
    /// scores zero.
    pub stability_cents: f32,
    /// A frame the detector was less sure of than this does not vote.
    pub min_confidence: f32,
    /// The share of a note that counts as fully held. Not 1.0:
    /// consonants and breaths eat the ends of every real note, and
    /// asking for every millisecond would make a perfect performance
    /// impossible.
    pub coverage_target: f32,
    /// Weight of pitch accuracy in a note's score.
    pub weight_pitch: f32,
    /// Weight of onset timing.
    pub weight_timing: f32,
    /// Weight of how much of the note was held.
    pub weight_coverage: f32,
    /// Weight of steadiness.
    pub weight_stability: f32,
    /// Points a flawless note is worth before the multiplier.
    pub points_per_note: u32,
}

impl Default for VocalScoreConfig {
    /// The plan's `Normal` values. ⚠️ They are a **starting point**,
    /// not a measured result: the plan asks for a recorded corpus to
    /// tune them against real singers before release, and that has
    /// not happened.
    fn default() -> VocalScoreConfig {
        VocalScoreConfig {
            mode: PitchMode::OctaveIndependent,
            perfect_cents: 20.0,
            great_cents: 40.0,
            good_cents: 70.0,
            weak_cents: 100.0,
            timing_window_s: 0.25,
            stability_cents: 50.0,
            min_confidence: 0.5,
            coverage_target: 0.8,
            weight_pitch: 0.65,
            weight_timing: 0.15,
            weight_coverage: 0.15,
            weight_stability: 0.05,
            points_per_note: 100,
        }
    }
}

impl VocalScoreConfig {
    /// The default with its pitch tolerances scaled for a difficulty.
    ///
    /// Only the cents move. Timing, hold and steadiness are about
    /// singing at all rather than about precision, and tightening
    /// them would make Expert a breath-control exercise.
    #[must_use]
    pub fn for_difficulty(difficulty: Difficulty) -> VocalScoreConfig {
        let scale = match difficulty {
            Difficulty::Easy => 1.4,
            Difficulty::Medium => 1.0,
            Difficulty::Hard => 0.8,
            Difficulty::Expert => 0.65,
        };
        let base = VocalScoreConfig::default();
        VocalScoreConfig {
            perfect_cents: base.perfect_cents * scale,
            great_cents: base.great_cents * scale,
            good_cents: base.good_cents * scale,
            weak_cents: base.weak_cents * scale,
            ..base
        }
    }

    /// The grade a mean absolute pitch error earns.
    #[must_use]
    pub fn grade_for_cents(&self, cents: f32) -> VocalGrade {
        let cents = cents.abs();
        if cents <= self.perfect_cents {
            VocalGrade::Perfect
        } else if cents <= self.great_cents {
            VocalGrade::Great
        } else if cents <= self.good_cents {
            VocalGrade::Good
        } else if cents <= self.weak_cents {
            VocalGrade::Weak
        } else {
            VocalGrade::Miss
        }
    }

    /// The grade a note's overall score earns, for the parts of a
    /// note that are not its tuning.
    #[must_use]
    pub fn grade_for_score(&self, score: f32) -> VocalGrade {
        if score >= 0.90 {
            VocalGrade::Perfect
        } else if score >= 0.75 {
            VocalGrade::Great
        } else if score >= 0.55 {
            VocalGrade::Good
        } else if score >= 0.30 {
            VocalGrade::Weak
        } else {
            VocalGrade::Miss
        }
    }
}

/// How one note went.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteOutcome {
    /// What it earns.
    pub grade: VocalGrade,
    /// What kind of note it was.
    pub kind: VocalKind,
    /// Mean absolute distance from the target, in cents. `None` for
    /// an unpitched note, or one nothing was sung into.
    pub mean_abs_cents: Option<f32>,
    /// How far the singer's onset was from the note's start, in
    /// seconds; positive is late. `None` when nothing was sung.
    pub onset_error_s: Option<f32>,
    /// The share of the note that was held, `0..=1`.
    pub coverage: f32,
    /// Steadiness, `0..=1`. `None` for an unpitched note.
    pub stability: Option<f32>,
    /// The note's overall score, `0..=1`.
    pub score: f32,
    /// Points earned, before any multiplier.
    pub points: u32,
}

/// How one phrase went.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PhraseOutcome {
    /// Which phrase.
    pub index: usize,
    /// Its score, `0..=1`, weighted by its notes' lengths.
    pub score: f32,
    /// The grade the score earns.
    pub grade: VocalGrade,
    /// How many notes it asked for.
    pub notes: usize,
    /// How many were hit.
    pub notes_hit: usize,
    /// Points awarded, multiplier included.
    pub points: u32,
    /// The phrase streak after this one.
    pub streak: u32,
    /// The multiplier this phrase was paid at.
    pub multiplier: u32,
}

/// Something the session decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VocalEvent {
    /// A note finished.
    Note {
        /// Which phrase it was in.
        phrase: usize,
        /// Its index inside that phrase.
        note: usize,
        /// How it went.
        outcome: NoteOutcome,
    },
    /// A phrase finished.
    Phrase(PhraseOutcome),
}

/// What a singer has done so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VocalPerformance {
    /// Points.
    pub score: u32,
    /// Phrases hit in a row.
    pub streak: u32,
    /// The longest such run.
    pub best_streak: u32,
    /// Notes the chart asked for, so far.
    pub notes: usize,
    /// Notes that were not a Miss.
    pub notes_hit: usize,
    /// Phrases finished.
    pub phrases: usize,
    /// … of which were Perfect.
    pub perfect_phrases: usize,
    /// The range actually sung.
    pub range: Option<VocalRange>,
    sum_pitch: f64,
    sum_timing: f64,
    sum_coverage: f64,
    sum_stability: f64,
    weight_pitch: f64,
    weight_other: f64,
    abs_cents_sum: f64,
    abs_cents_weight: f64,
}

impl VocalPerformance {
    /// Mean pitch accuracy over the pitched notes, `0..=1`. `None`
    /// when nothing pitched has been judged.
    #[must_use]
    pub fn pitch_accuracy(&self) -> Option<f32> {
        mean(self.sum_pitch, self.weight_pitch)
    }

    /// Mean onset accuracy, `0..=1`.
    #[must_use]
    pub fn timing_accuracy(&self) -> Option<f32> {
        mean(self.sum_timing, self.weight_other)
    }

    /// Mean hold, `0..=1`.
    #[must_use]
    pub fn coverage(&self) -> Option<f32> {
        mean(self.sum_coverage, self.weight_other)
    }

    /// Mean steadiness over the pitched notes, `0..=1`.
    #[must_use]
    pub fn stability(&self) -> Option<f32> {
        mean(self.sum_stability, self.weight_pitch)
    }

    /// Mean absolute pitch error in cents, over the pitched notes
    /// that were sung into.
    #[must_use]
    pub fn mean_abs_cents(&self) -> Option<f32> {
        mean(self.abs_cents_sum, self.abs_cents_weight)
    }

    /// The run as one number, `0..=1`, under the same weighting a
    /// single note is scored by.
    ///
    /// A component nobody produced — pitch on an all-rap part, or
    /// anything at all on a run where the microphone never worked —
    /// leaves the denominator instead of scoring zero. A part that
    /// was never sung is [`None`], which is a different thing from
    /// having sung it badly.
    #[must_use]
    pub fn overall(&self, config: &VocalScoreConfig) -> Option<f32> {
        let mut total = 0.0f32;
        let mut weight = 0.0f32;
        let mut take = |value: Option<f32>, w: f32| {
            if let Some(value) = value {
                total += value * w;
                weight += w;
            }
        };
        take(self.pitch_accuracy(), config.weight_pitch);
        take(self.timing_accuracy(), config.weight_timing);
        take(self.coverage(), config.weight_coverage);
        take(self.stability(), config.weight_stability);
        (weight > 0.0).then(|| (total / weight).clamp(0.0, 1.0))
    }

    /// The grade the run earns as a whole.
    #[must_use]
    pub fn grade(&self, config: &VocalScoreConfig) -> VocalGrade {
        self.overall(config)
            .map_or(VocalGrade::Miss, |score| config.grade_for_score(score))
    }

    /// The share of asked-for notes that were hit.
    #[must_use]
    pub fn hit_rate(&self) -> f32 {
        if self.notes == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "note counts are thousands at most"
        )]
        let rate = self.notes_hit as f32 / self.notes as f32;
        rate
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a mean of values in 0..=1, or of cents; f32 is what callers show"
)]
fn mean(sum: f64, weight: f64) -> Option<f32> {
    (weight > 0.0).then(|| (sum / weight) as f32)
}

/// The multiplier a phrase streak pays at.
///
/// The same shape the guitar side's combo has — four steps, capped —
/// so a run at the microphone feels like a run on the neck. ⚠️ The
/// thresholds are a starting point and belong in the tuning pass with
/// everything else in [`VocalScoreConfig`].
#[must_use]
pub fn multiplier(streak: u32) -> u32 {
    match streak {
        0..=3 => 1,
        4..=7 => 2,
        8..=11 => 3,
        _ => 4,
    }
}

/// What one note has collected so far.
#[derive(Debug, Default, Clone)]
struct NoteAcc {
    voiced_s: f64,
    cents_abs_sum: f64,
    cents_sum: f64,
    cents_sq_sum: f64,
    cents_weight: f64,
    first_voiced_s: Option<f64>,
    low: Option<f32>,
    high: Option<f32>,
}

/// The deterministic judgment engine for one singer.
pub struct VocalSession {
    part: VocalPart,
    config: VocalScoreConfig,
    phrase: usize,
    note: usize,
    acc: NoteAcc,
    phrase_notes: Vec<(NoteOutcome, f64)>,
    performance: VocalPerformance,
    last_time_s: Option<f64>,
    /// Nominal seconds a frame stands for, until two frames say
    /// otherwise.
    nominal_frame_s: f64,
}

impl VocalSession {
    /// A session holding a singer to `part`.
    #[must_use]
    pub fn new(part: VocalPart, config: VocalScoreConfig) -> VocalSession {
        VocalSession {
            part,
            config,
            phrase: 0,
            note: 0,
            acc: NoteAcc::default(),
            phrase_notes: Vec::new(),
            performance: VocalPerformance::default(),
            last_time_s: None,
            nominal_frame_s: 0.016,
        }
    }

    /// What the singer is being held to.
    #[must_use]
    pub fn part(&self) -> &VocalPart {
        &self.part
    }

    /// The numbers it judges on.
    #[must_use]
    pub fn config(&self) -> &VocalScoreConfig {
        &self.config
    }

    /// What has happened so far.
    #[must_use]
    pub fn performance(&self) -> &VocalPerformance {
        &self.performance
    }

    /// The note being sung at the last frame's time, if any.
    #[must_use]
    pub fn active_note(&self) -> Option<&VocalNote> {
        self.part.phrases.get(self.phrase)?.notes.get(self.note)
    }

    /// Take one frame of microphone pitch.
    ///
    /// Frames must arrive in song-time order; anything older than the
    /// last one is dropped rather than judged, because a frame that
    /// travels backwards through a note would be counted twice.
    pub fn feed(&mut self, frame: VocalInputFrame, out: &mut Vec<VocalEvent>) {
        if self
            .last_time_s
            .is_some_and(|last| frame.song_time_s < last)
        {
            return;
        }
        let span = self.last_time_s.map_or(self.nominal_frame_s, |last| {
            // A gap longer than a tenth of a second is a stall,
            // not one frame's worth of singing. Counting it as
            // held time would pay a singer for a dropout.
            (frame.song_time_s - last).clamp(0.0, 0.1)
        });
        self.last_time_s = Some(frame.song_time_s);
        self.close_finished(frame.song_time_s, out);
        self.accumulate(&frame, span);
    }

    /// Close everything the song has moved past. Call at the end of a
    /// run, so the last phrase is scored like the rest.
    pub fn finish(&mut self, out: &mut Vec<VocalEvent>) {
        self.close_finished(f64::INFINITY, out);
    }

    fn close_finished(&mut self, now: f64, out: &mut Vec<VocalEvent>) {
        loop {
            // The note is CLONED out before it is judged: judging
            // writes to the performance, and a borrow of the chart
            // held across that would be a borrow of the same `self`.
            // A note's contour is a handful of points and this runs
            // once per note, not per frame.
            let Some((phrase_end, note_count)) = self
                .part
                .phrases
                .get(self.phrase)
                .map(|phrase| (phrase.end_s, phrase.notes.len()))
            else {
                return;
            };
            while let Some(note) = self
                .part
                .phrases
                .get(self.phrase)
                .and_then(|phrase| phrase.notes.get(self.note))
                .cloned()
            {
                if now < note.end_s {
                    break;
                }
                let outcome = self.judge_note(&note);
                out.push(VocalEvent::Note {
                    phrase: self.phrase,
                    note: self.note,
                    outcome,
                });
                self.phrase_notes.push((outcome, note.duration_s()));
                self.acc = NoteAcc::default();
                self.note += 1;
            }
            // A phrase closes only once the song has passed its end
            // AND every note in it has been judged — a phrase whose
            // last note is still open is still being sung.
            if now < phrase_end || self.note < note_count {
                return;
            }
            let outcome = self.close_phrase();
            out.push(VocalEvent::Phrase(outcome));
            self.phrase += 1;
            self.note = 0;
        }
    }

    fn accumulate(&mut self, frame: &VocalInputFrame, span: f64) {
        let Some(note) = self
            .part
            .phrases
            .get(self.phrase)
            .and_then(|p| p.notes.get(self.note))
        else {
            return;
        };
        if !note.contains(frame.song_time_s) {
            return;
        }
        // Loudness and confidence decide whether a frame votes at
        // all. They never add to a score.
        if !frame.voiced || frame.confidence < self.config.min_confidence {
            return;
        }
        self.acc.voiced_s += span;
        if self.acc.first_voiced_s.is_none() {
            self.acc.first_voiced_s = Some(frame.song_time_s);
        }
        let Some(midi) = frame.midi else {
            return;
        };
        self.acc.low = Some(self.acc.low.map_or(midi, |low| low.min(midi)));
        self.acc.high = Some(self.acc.high.map_or(midi, |high| high.max(midi)));
        let Some(target) = note.target_at(frame.song_time_s) else {
            return;
        };
        // A clipped frame's pitch is not trustworthy, so it holds the
        // note but does not tune it.
        if frame.clipped {
            return;
        }
        let cents = f64::from(self.config.mode.error_cents(midi, target));
        self.acc.cents_sum += cents * span;
        self.acc.cents_abs_sum += cents.abs() * span;
        self.acc.cents_sq_sum += cents * cents * span;
        self.acc.cents_weight += span;
    }

    fn judge_note(&mut self, note: &VocalNote) -> NoteOutcome {
        let config = &self.config;
        let acc = &self.acc;
        let duration = note.duration_s().max(1e-6);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a share in 0..=1 from seconds"
        )]
        let coverage = ((acc.voiced_s / (duration * f64::from(config.coverage_target))) as f32)
            .clamp(0.0, 1.0);

        #[expect(
            clippy::cast_possible_truncation,
            reason = "seconds inside one note; f32 is what callers show"
        )]
        let onset_error_s = acc
            .first_voiced_s
            .map(|first| (first - note.start_s) as f32);
        let timing = onset_error_s.map_or(0.0, |error| {
            1.0 - (error.abs() / config.timing_window_s.max(1e-6)).clamp(0.0, 1.0)
        });

        let pitched = note.kind.is_pitched();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "cents; f32 is the unit everything else uses"
        )]
        let mean_abs_cents = (pitched && acc.cents_weight > 0.0)
            .then(|| (acc.cents_abs_sum / acc.cents_weight) as f32);
        let stability = (pitched && acc.cents_weight > 0.0).then(|| {
            let mean = acc.cents_sum / acc.cents_weight;
            let variance = (acc.cents_sq_sum / acc.cents_weight - mean * mean).max(0.0);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a spread in cents; f32 is the unit"
            )]
            let spread = variance.sqrt() as f32;
            1.0 - (spread / config.stability_cents.max(1e-6)).clamp(0.0, 1.0)
        });
        let pitch = mean_abs_cents.map_or(0.0, |cents| {
            1.0 - (cents / config.weak_cents.max(1e-6)).clamp(0.0, 1.0)
        });

        // How much of the note was actually delivered, tuning aside:
        // was it started on time and held. This is the whole score of
        // an unpitched note, and the CAP on a pitched one.
        //
        // ⚠️ It has to be the cap rather than the total score, and
        // the first version got that wrong. Capping by the total —
        // which already contains pitch — made the cent bands almost
        // meaningless: a note 15 cents out is Perfect by the table
        // and came back Great, because its pitch term had already
        // pulled the total under the Perfect threshold. The cap
        // exists for one thing only, and it is not tuning: a note
        // perfectly in tune for a third of its length is not a
        // Perfect.
        let delivery_total = config.weight_timing + config.weight_coverage;
        let delivery = (config.weight_timing * timing + config.weight_coverage * coverage)
            / delivery_total.max(1e-6);

        // Rap and speech drop pitch and stability out of the
        // denominator rather than scoring zero in them.
        let (score, grade) = if pitched {
            let total = config.weight_pitch
                + config.weight_timing
                + config.weight_coverage
                + config.weight_stability;
            let score = (config.weight_pitch * pitch
                + config.weight_timing * timing
                + config.weight_coverage * coverage
                + config.weight_stability * stability.unwrap_or(0.0))
                / total.max(1e-6);
            let grade = match mean_abs_cents {
                // Nothing was sung into it at all.
                None => VocalGrade::Miss,
                Some(cents) => config
                    .grade_for_cents(cents)
                    .worse_of(config.grade_for_score(delivery)),
            };
            (score, grade)
        } else {
            let grade = if acc.voiced_s <= 0.0 {
                VocalGrade::Miss
            } else {
                config.grade_for_score(delivery)
            };
            (delivery, grade)
        };
        let score = score.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "points from a share of a small positive constant"
        )]
        let points = (score * config.points_per_note as f32).round() as u32;

        // The range the singer actually reached, whatever the chart
        // asked for — it is a fact about them, not about the song.
        if let (Some(low), Some(high)) = (acc.low, acc.high) {
            let sung = VocalRange {
                low_midi: low,
                high_midi: high,
            };
            self.performance.range = Some(
                self.performance
                    .range
                    .map_or(sung, |range| range.union(sung)),
            );
        }
        self.performance.notes += 1;
        if grade.hit() {
            self.performance.notes_hit += 1;
        }
        let weight = duration;
        self.performance.sum_timing += f64::from(timing) * weight;
        self.performance.sum_coverage += f64::from(coverage) * weight;
        self.performance.weight_other += weight;
        if pitched {
            self.performance.sum_pitch += f64::from(pitch) * weight;
            self.performance.sum_stability += f64::from(stability.unwrap_or(0.0)) * weight;
            self.performance.weight_pitch += weight;
            if let Some(cents) = mean_abs_cents {
                self.performance.abs_cents_sum += f64::from(cents) * weight;
                self.performance.abs_cents_weight += weight;
            }
        }

        NoteOutcome {
            grade,
            kind: note.kind,
            mean_abs_cents,
            onset_error_s,
            coverage,
            stability,
            score,
            points,
        }
    }

    fn close_phrase(&mut self) -> PhraseOutcome {
        let notes = std::mem::take(&mut self.phrase_notes);
        let total_weight: f64 = notes.iter().map(|(_, d)| *d).sum();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a weighted mean of values in 0..=1"
        )]
        let score = if total_weight > 0.0 {
            (notes
                .iter()
                .map(|(o, d)| f64::from(o.score) * d)
                .sum::<f64>()
                / total_weight) as f32
        } else {
            0.0
        };
        let grade = self.config.grade_for_score(score);
        let notes_hit = notes.iter().filter(|(o, _)| o.grade.hit()).count();
        // A phrase counts for the streak when it was sung at all —
        // the streak is about staying with the song, not about being
        // note-perfect, which is what the grade is for.
        if grade.hit() {
            self.performance.streak += 1;
            self.performance.best_streak =
                self.performance.best_streak.max(self.performance.streak);
        } else {
            self.performance.streak = 0;
        }
        let multiplier = multiplier(self.performance.streak.saturating_sub(1));
        let base: u32 = notes.iter().map(|(o, _)| o.points).sum();
        let points = base.saturating_mul(multiplier);
        self.performance.score = self.performance.score.saturating_add(points);
        self.performance.phrases += 1;
        if grade == VocalGrade::Perfect {
            self.performance.perfect_phrases += 1;
        }
        PhraseOutcome {
            index: self.phrase,
            score,
            grade,
            notes: notes.len(),
            notes_hit,
            points,
            streak: self.performance.streak,
            multiplier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocal::{VocalPhrase, VocalPitchPoint, VocalRole};

    const HOP: f64 = 0.016;

    fn note(start: f64, end: f64, midi: f32) -> VocalNote {
        VocalNote {
            start_s: start,
            end_s: end,
            kind: VocalKind::Pitched,
            target_midi: Some(midi),
            contour: Vec::new(),
            confidence: 1.0,
            token_range: None,
        }
    }

    fn part(notes: Vec<VocalNote>) -> VocalPart {
        let start = notes.first().map_or(0.0, |n| n.start_s);
        let end = notes.last().map_or(1.0, |n| n.end_s);
        VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![VocalPhrase {
                start_s: start,
                end_s: end,
                confidence: 1.0,
                tokens: Vec::new(),
                notes,
            }],
        }
    }

    /// How a singer behaves for one run.
    #[derive(Clone, Copy)]
    struct Singer {
        /// Semitones off the target.
        offset: f32,
        /// Share of each note actually sung, from its start.
        coverage: f32,
        /// Seconds late the first frame of each note is.
        late_s: f64,
        /// What the detector reports for every frame.
        confidence: f32,
        /// The level reported. Judgment must not read it.
        rms_dbfs: f32,
        /// Wobble amplitude in semitones, alternating frame to frame.
        wobble: f32,
    }

    impl Default for Singer {
        fn default() -> Singer {
            Singer {
                offset: 0.0,
                coverage: 1.0,
                late_s: 0.0,
                confidence: 0.9,
                rms_dbfs: -18.0,
                wobble: 0.0,
            }
        }
    }

    /// Run a whole part through a session with one singer's habits.
    fn sing(
        part: &VocalPart,
        singer: Singer,
        config: VocalScoreConfig,
    ) -> (VocalPerformance, Vec<VocalEvent>) {
        let mut session = VocalSession::new(part.clone(), config);
        let mut events = Vec::new();
        let (from, to) = part.span_s().expect("a part with phrases");
        let mut t = from - 0.2;
        let mut step = 0u32;
        while t < to + 0.2 {
            let inside = part.note_at(t).and_then(|n| {
                let sung_from = n.start_s + singer.late_s;
                let sung_to = n.start_s + n.duration_s() * f64::from(singer.coverage);
                if t < sung_from || t >= sung_to {
                    return None;
                }
                // An unpitched note has no target, and a singer still
                // makes a sound at it — the helper said otherwise and
                // the rap test was measuring silence.
                Some(n.target_at(t).unwrap_or(60.0))
            });
            let midi = inside.map(|target| {
                let wobble = if step.is_multiple_of(2) {
                    singer.wobble
                } else {
                    -singer.wobble
                };
                target + singer.offset + wobble
            });
            session.feed(
                VocalInputFrame {
                    song_time_s: t,
                    midi,
                    confidence: if midi.is_some() {
                        singer.confidence
                    } else {
                        0.0
                    },
                    rms_dbfs: if midi.is_some() {
                        singer.rms_dbfs
                    } else {
                        -90.0
                    },
                    voiced: midi.is_some(),
                    clipped: false,
                },
                &mut events,
            );
            t += HOP;
            step += 1;
        }
        session.finish(&mut events);
        (session.performance().clone(), events)
    }

    #[test]
    fn singing_it_right_scores_everything() {
        let p = part(vec![note(1.0, 2.0, 60.0), note(2.5, 3.5, 64.0)]);
        let (perf, events) = sing(&p, Singer::default(), VocalScoreConfig::default());
        assert_eq!(perf.notes, 2);
        assert_eq!(perf.notes_hit, 2);
        assert_eq!(perf.phrases, 1);
        assert_eq!(perf.perfect_phrases, 1, "dead on is a perfect phrase");
        assert!(perf.score > 0, "a perfect run scored nothing");
        assert!(perf.pitch_accuracy().expect("pitched notes") > 0.99);
        assert!(perf.mean_abs_cents().expect("sung") < 1.0);
        for event in &events {
            if let VocalEvent::Note { outcome, .. } = event {
                assert_eq!(outcome.grade, VocalGrade::Perfect, "{outcome:?}");
            }
        }
    }

    #[test]
    fn singing_nothing_scores_nothing_and_says_so() {
        let p = part(vec![note(1.0, 2.0, 60.0), note(2.5, 3.5, 64.0)]);
        let silent = Singer {
            coverage: 0.0,
            ..Singer::default()
        };
        let (perf, events) = sing(&p, silent, VocalScoreConfig::default());
        assert_eq!(perf.score, 0);
        assert_eq!(perf.notes_hit, 0);
        assert_eq!(perf.streak, 0);
        let notes: Vec<&NoteOutcome> = events
            .iter()
            .filter_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(outcome),
                VocalEvent::Phrase(_) => None,
            })
            .collect();
        assert_eq!(notes.len(), 2);
        for outcome in notes {
            assert_eq!(outcome.grade, VocalGrade::Miss);
            assert_eq!(outcome.mean_abs_cents, None, "nothing was sung to measure");
            assert_eq!(outcome.onset_error_s, None);
            assert_eq!(outcome.coverage, 0.0);
        }
    }

    #[test]
    fn the_tolerances_are_where_the_config_says_they_are() {
        let p = part(vec![note(1.0, 2.0, 60.0)]);
        let config = VocalScoreConfig::default();
        // Just inside each band, in semitones (100 cents each).
        for (semitones, expected) in [
            (0.15f32, VocalGrade::Perfect),
            (0.35, VocalGrade::Great),
            (0.65, VocalGrade::Good),
            (0.95, VocalGrade::Weak),
            (1.50, VocalGrade::Miss),
        ] {
            let singer = Singer {
                offset: semitones,
                ..Singer::default()
            };
            let (_, events) = sing(&p, singer, config);
            let grade = events
                .iter()
                .find_map(|e| match e {
                    VocalEvent::Note { outcome, .. } => Some(outcome.grade),
                    VocalEvent::Phrase(_) => None,
                })
                .expect("one note");
            assert_eq!(
                grade,
                expected,
                "{} cents should be {expected:?}, got {grade:?}",
                semitones * 100.0
            );
        }
    }

    #[test]
    fn a_harder_difficulty_asks_for_better_tuning_and_nothing_else() {
        let easy = VocalScoreConfig::for_difficulty(Difficulty::Easy);
        let expert = VocalScoreConfig::for_difficulty(Difficulty::Expert);
        assert!(expert.perfect_cents < easy.perfect_cents);
        assert!((easy.perfect_cents - 28.0).abs() < 1e-3);
        assert!((expert.perfect_cents - 13.0).abs() < 1e-3);
        // Hold and timing are about singing at all, not precision.
        assert_eq!(easy.timing_window_s, expert.timing_window_s);
        assert_eq!(easy.coverage_target, expert.coverage_target);

        // And the difficulty really changes the verdict on one run.
        let p = part(vec![note(1.0, 2.0, 60.0)]);
        // 27 cents: inside Easy's Perfect band (28) and outside
        // Expert's Great band (26).
        let singer = Singer {
            offset: 0.27,
            ..Singer::default()
        };
        let grade_of = |config| {
            sing(&p, singer, config)
                .1
                .iter()
                .find_map(|e| match e {
                    VocalEvent::Note { outcome, .. } => Some(outcome.grade),
                    VocalEvent::Phrase(_) => None,
                })
                .expect("one note")
        };
        assert_eq!(grade_of(easy), VocalGrade::Perfect);
        assert_eq!(grade_of(expert), VocalGrade::Good);
    }

    #[test]
    fn an_octave_out_is_forgiven_by_default_and_not_in_strict_mode() {
        let p = part(vec![note(1.0, 2.0, 69.0)]);
        let low = Singer {
            offset: -12.0,
            ..Singer::default()
        };
        let (forgiving, _) = sing(&p, low, VocalScoreConfig::default());
        assert_eq!(forgiving.notes_hit, 1, "a man singing a woman's part");
        assert!(forgiving.mean_abs_cents().expect("sung") < 1.0);

        let strict = VocalScoreConfig {
            mode: PitchMode::Strict,
            ..VocalScoreConfig::default()
        };
        let (exact, _) = sing(&p, low, strict);
        assert_eq!(exact.notes_hit, 0, "strict counts every cent of the octave");
    }

    #[test]
    fn loudness_is_never_a_reward() {
        // The same performance, twice, at a whisper and at a shout.
        // Anything but an identical score means volume leaked in.
        let p = part(vec![note(1.0, 2.0, 60.0), note(2.2, 3.0, 62.0)]);
        let quiet = Singer {
            rms_dbfs: -40.0,
            ..Singer::default()
        };
        let loud = Singer {
            rms_dbfs: -3.0,
            ..Singer::default()
        };
        let (a, _) = sing(&p, quiet, VocalScoreConfig::default());
        let (b, _) = sing(&p, loud, VocalScoreConfig::default());
        assert_eq!(a.score, b.score);
        assert_eq!(a.mean_abs_cents(), b.mean_abs_cents());
    }

    #[test]
    fn a_frame_the_detector_is_unsure_of_does_not_vote() {
        let p = part(vec![note(1.0, 2.0, 60.0)]);
        let unsure = Singer {
            confidence: 0.2,
            ..Singer::default()
        };
        let (perf, _) = sing(&p, unsure, VocalScoreConfig::default());
        assert_eq!(perf.notes_hit, 0, "a guess was taken as singing");
        assert_eq!(perf.mean_abs_cents(), None);
    }

    #[test]
    fn coming_in_late_costs_timing_but_not_the_note() {
        let p = part(vec![note(1.0, 3.0, 60.0)]);
        let late = Singer {
            late_s: 0.20,
            ..Singer::default()
        };
        let (_, events) = sing(&p, late, VocalScoreConfig::default());
        let outcome = events
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(*outcome),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        let error = outcome.onset_error_s.expect("something was sung");
        assert!((error - 0.20).abs() < 0.03, "onset error read as {error}");
        assert!(outcome.grade.hit(), "the note was sung, only late");
        assert!(
            outcome.score < 0.95,
            "being late cost nothing: {}",
            outcome.score
        );
        // On time, the same singing scores better.
        let (_, on_time) = sing(&p, Singer::default(), VocalScoreConfig::default());
        let better = on_time
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(outcome.score),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        assert!(better > outcome.score);
    }

    #[test]
    fn letting_go_early_costs_the_hold() {
        let p = part(vec![note(1.0, 3.0, 60.0)]);
        let short = Singer {
            coverage: 0.4,
            ..Singer::default()
        };
        let (_, events) = sing(&p, short, VocalScoreConfig::default());
        let outcome = events
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(*outcome),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        assert!(
            (outcome.coverage - 0.5).abs() < 0.08,
            "40 % of the note against an 80 % target is half: {}",
            outcome.coverage
        );
        // In tune throughout, so the pitch grade is Perfect — and the
        // note still must not be, because most of it was not sung.
        assert!(
            outcome.grade < VocalGrade::Perfect,
            "a note held two fifths of the way graded {:?}",
            outcome.grade
        );
    }

    #[test]
    fn a_wobbling_voice_loses_steadiness_but_keeps_its_average() {
        let p = part(vec![note(1.0, 3.0, 60.0)]);
        // Alternating +-40 cents: the mean is dead on, the spread is
        // not. Only stability may notice.
        let wobbly = Singer {
            wobble: 0.40,
            ..Singer::default()
        };
        let (_, events) = sing(&p, wobbly, VocalScoreConfig::default());
        let outcome = events
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(*outcome),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        let stability = outcome.stability.expect("a pitched note");
        assert!(
            stability < 0.3,
            "a 40-cent wobble read as steady: {stability}"
        );
        let steady = sing(&p, Singer::default(), VocalScoreConfig::default())
            .1
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => outcome.stability,
                VocalEvent::Phrase(_) => None,
            })
            .expect("a pitched note");
        assert!(steady > 0.95, "a steady note read as unsteady: {steady}");
    }

    #[test]
    fn rap_is_judged_on_delivery_and_never_on_pitch() {
        // The plan's rule: rap has no fundamental to be wrong about.
        let mut rap = note(1.0, 2.0, 60.0);
        rap.kind = VocalKind::Rap;
        rap.target_midi = None;
        let p = part(vec![rap]);
        // Wildly "out of tune" — and on time, and held.
        let tuneless = Singer {
            offset: 7.0,
            wobble: 3.0,
            ..Singer::default()
        };
        let (perf, events) = sing(&p, tuneless, VocalScoreConfig::default());
        let outcome = events
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(*outcome),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        assert_eq!(outcome.grade, VocalGrade::Perfect, "{outcome:?}");
        assert_eq!(outcome.mean_abs_cents, None, "rap has no pitch error");
        assert_eq!(outcome.stability, None);
        assert_eq!(
            perf.pitch_accuracy(),
            None,
            "a rapped run must not report a pitch accuracy"
        );
        // Saying nothing is still a miss.
        let silent = Singer {
            coverage: 0.0,
            ..Singer::default()
        };
        assert_eq!(sing(&p, silent, VocalScoreConfig::default()).0.notes_hit, 0);
    }

    #[test]
    fn a_bend_is_followed_rather_than_averaged() {
        // A singer who follows the contour beats one who holds the
        // note's nominal pitch through it.
        let mut bend = note(1.0, 3.0, 61.0);
        bend.contour = vec![
            VocalPitchPoint {
                offset_s: 0.0,
                midi: 60.0,
                confidence: 1.0,
            },
            VocalPitchPoint {
                offset_s: 2.0,
                midi: 62.0,
                confidence: 1.0,
            },
        ];
        let p = part(vec![bend]);
        let (following, _) = sing(&p, Singer::default(), VocalScoreConfig::default());
        assert!(following.mean_abs_cents().expect("sung") < 2.0);
        assert_eq!(following.notes_hit, 1);
    }

    #[test]
    fn the_streak_pays_a_multiplier_and_a_missed_phrase_takes_it_away() {
        // Twelve phrases in a row, then one nobody sings.
        let mut phrases = Vec::new();
        for i in 0..13u32 {
            let start = f64::from(i) * 2.0;
            phrases.push(VocalPhrase {
                start_s: start,
                end_s: start + 1.0,
                confidence: 1.0,
                tokens: Vec::new(),
                notes: vec![note(start, start + 1.0, 60.0)],
            });
        }
        let p = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases,
        };
        let (perf, events) = sing(&p, Singer::default(), VocalScoreConfig::default());
        let multipliers: Vec<u32> = events
            .iter()
            .filter_map(|e| match e {
                VocalEvent::Phrase(outcome) => Some(outcome.multiplier),
                VocalEvent::Note { .. } => None,
            })
            .collect();
        assert_eq!(multipliers.len(), 13);
        assert_eq!(multipliers[0], 1, "the first phrase pays flat");
        assert_eq!(multipliers[4], 2);
        assert_eq!(multipliers[8], 3);
        assert_eq!(multipliers[12], 4, "and it caps");
        assert_eq!(perf.best_streak, 13);

        // The ladder itself, at its edges.
        assert_eq!(multiplier(0), 1);
        assert_eq!(multiplier(3), 1);
        assert_eq!(multiplier(4), 2);
        assert_eq!(multiplier(11), 3);
        assert_eq!(multiplier(12), 4);
        assert_eq!(multiplier(9_999), 4);
    }

    #[test]
    fn a_dropped_phrase_resets_the_streak() {
        let mut phrases = Vec::new();
        for i in 0..6u32 {
            let start = f64::from(i) * 2.0;
            phrases.push(VocalPhrase {
                start_s: start,
                end_s: start + 1.0,
                confidence: 1.0,
                tokens: Vec::new(),
                notes: vec![note(start, start + 1.0, 60.0)],
            });
        }
        let p = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases,
        };
        // Sing everything, but go silent for the fourth phrase by
        // feeding frames by hand.
        let mut session = VocalSession::new(p.clone(), VocalScoreConfig::default());
        let mut events = Vec::new();
        let mut t = 0.0f64;
        while t < 12.0 {
            let target = p.note_at(t).and_then(|n| n.target_at(t));
            let quiet = (6.0..8.0).contains(&t);
            let midi = if quiet { None } else { target };
            session.feed(
                VocalInputFrame {
                    song_time_s: t,
                    midi,
                    confidence: if midi.is_some() { 0.9 } else { 0.0 },
                    rms_dbfs: -18.0,
                    voiced: midi.is_some(),
                    clipped: false,
                },
                &mut events,
            );
            t += HOP;
        }
        session.finish(&mut events);
        let streaks: Vec<u32> = events
            .iter()
            .filter_map(|e| match e {
                VocalEvent::Phrase(outcome) => Some(outcome.streak),
                VocalEvent::Note { .. } => None,
            })
            .collect();
        assert_eq!(streaks, vec![1, 2, 3, 0, 1, 2], "{streaks:?}");
        assert_eq!(session.performance().best_streak, 3);
    }

    #[test]
    fn a_clock_that_snaps_backwards_does_not_pay_for_the_same_second_twice() {
        // The song clock steps backwards when it reconciles against
        // the audio device, and then re-advances over ground it has
        // already covered. Without the guard that whole replayed
        // stretch is counted a second time and a singer is paid for
        // holding a note they held once.
        //
        // ⚠️ A single rewound frame is NOT enough to show this: its
        // own span clamps to zero, so the first version of this test
        // passed with the guard removed. It takes a rewind followed
        // by a replay, which is what a snap actually looks like.
        let frame = |t: f64| VocalInputFrame {
            song_time_s: t,
            midi: Some(60.0),
            confidence: 0.9,
            rms_dbfs: -18.0,
            voiced: true,
            clipped: false,
        };
        let coverage_of = |times: &[f64]| {
            let mut session = VocalSession::new(
                part(vec![note(1.0, 3.0, 60.0)]),
                VocalScoreConfig::default(),
            );
            let mut events = Vec::new();
            for &t in times {
                session.feed(frame(t), &mut events);
            }
            session.finish(&mut events);
            events
                .iter()
                .find_map(|e| match e {
                    VocalEvent::Note { outcome, .. } => Some(outcome.coverage),
                    VocalEvent::Phrase(_) => None,
                })
                .expect("one note")
        };
        // Straight through, 50 ms apart.
        let forward: Vec<f64> = (0..20).map(|i| 1.0 + f64::from(i) * 0.05).collect();
        // The same run, but at the halfway point the clock snaps back
        // a quarter of a second and re-covers it.
        let mut snapped = forward[..10].to_vec();
        snapped.extend(forward[5..].iter().copied());

        let straight = coverage_of(&forward);
        let replayed = coverage_of(&snapped);
        assert!(
            (straight - replayed).abs() < 1e-6,
            "the replayed quarter second was paid for again: {replayed} against {straight}"
        );
    }

    #[test]
    fn a_dropout_is_not_paid_for_as_held_time() {
        // A ten-second gap between two frames must not count as ten
        // seconds of singing.
        let p = part(vec![note(0.0, 20.0, 60.0)]);
        let mut session = VocalSession::new(p, VocalScoreConfig::default());
        let mut events = Vec::new();
        for t in [0.1f64, 10.0, 19.0] {
            session.feed(
                VocalInputFrame {
                    song_time_s: t,
                    midi: Some(60.0),
                    confidence: 0.9,
                    rms_dbfs: -18.0,
                    voiced: true,
                    clipped: false,
                },
                &mut events,
            );
        }
        session.finish(&mut events);
        let outcome = events
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(*outcome),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        assert!(
            outcome.coverage < 0.05,
            "three frames bought {} of a twenty-second note",
            outcome.coverage
        );
    }

    #[test]
    fn a_clipped_frame_holds_the_note_but_does_not_tune_it() {
        // The level is what is wrong, not the note — but a clipped
        // window's pitch is not evidence either way.
        let p = part(vec![note(1.0, 2.0, 60.0)]);
        let mut session = VocalSession::new(p, VocalScoreConfig::default());
        let mut events = Vec::new();
        let mut t = 1.0;
        while t < 2.0 {
            session.feed(
                VocalInputFrame {
                    song_time_s: t,
                    // Badly out of tune, and clipped: it must not
                    // count against the tuning.
                    midi: Some(66.0),
                    confidence: 0.9,
                    rms_dbfs: -0.5,
                    voiced: true,
                    clipped: true,
                },
                &mut events,
            );
            t += HOP;
        }
        session.finish(&mut events);
        let outcome = events
            .iter()
            .find_map(|e| match e {
                VocalEvent::Note { outcome, .. } => Some(*outcome),
                VocalEvent::Phrase(_) => None,
            })
            .expect("one note");
        assert_eq!(
            outcome.mean_abs_cents, None,
            "a clipped frame tuned the note"
        );
        assert!(outcome.coverage > 0.9, "it should still hold the note");
    }

    #[test]
    fn the_session_reports_the_range_the_singer_actually_reached() {
        let p = part(vec![note(1.0, 2.0, 55.0), note(2.5, 3.5, 67.0)]);
        // A singer an octave down on everything: the range recorded
        // is theirs, not the chart's.
        let low = Singer {
            offset: -12.0,
            ..Singer::default()
        };
        let (perf, _) = sing(&p, low, VocalScoreConfig::default());
        let range = perf.range.expect("something was sung");
        assert!((range.low_midi - 43.0).abs() < 0.5, "{range:?}");
        assert!((range.high_midi - 55.0).abs() < 0.5, "{range:?}");
    }

    #[test]
    fn nothing_is_scored_until_a_phrase_is_finished_and_finish_closes_the_last() {
        let p = part(vec![note(1.0, 2.0, 60.0)]);
        let mut session = VocalSession::new(p, VocalScoreConfig::default());
        let mut events = Vec::new();
        let mut t = 1.0;
        while t < 1.9 {
            session.feed(
                VocalInputFrame {
                    song_time_s: t,
                    midi: Some(60.0),
                    confidence: 0.9,
                    rms_dbfs: -18.0,
                    voiced: true,
                    clipped: false,
                },
                &mut events,
            );
            t += HOP;
        }
        assert!(events.is_empty(), "a note still being sung was judged");
        assert_eq!(session.performance().score, 0);
        session.finish(&mut events);
        assert_eq!(events.len(), 2, "the note and its phrase");
        assert!(session.performance().score > 0);
    }

    #[test]
    fn a_run_sums_itself_up_without_counting_what_nobody_sang() {
        let config = VocalScoreConfig::default();
        let p = part(vec![note(1.0, 2.0, 60.0), note(2.5, 3.5, 64.0)]);
        let (good, _) = sing(&p, Singer::default(), config);
        let overall = good.overall(&config).expect("something was sung");
        assert!(overall > 0.95, "a flawless run summed to {overall}");
        assert_eq!(good.grade(&config), VocalGrade::Perfect);

        // An all-rap part has no pitch and no steadiness, and its
        // summary must not be dragged down by their absence.
        let mut rap = note(1.0, 2.0, 60.0);
        rap.kind = VocalKind::Rap;
        rap.target_midi = None;
        let (rapped, _) = sing(&part(vec![rap]), Singer::default(), config);
        assert_eq!(rapped.pitch_accuracy(), None);
        assert!(
            rapped.overall(&config).expect("delivered") > 0.95,
            "rap was penalised for having no pitch: {:?}",
            rapped.overall(&config)
        );

        // Nothing sung at all is not a bad performance — it is no
        // performance, and the two must not read the same.
        let silent = Singer {
            coverage: 0.0,
            ..Singer::default()
        };
        let (nothing, _) = sing(&p, silent, config);
        assert_eq!(nothing.grade(&config), VocalGrade::Miss);
        let never = VocalPerformance::default();
        assert_eq!(never.overall(&config), None, "an empty run has no score");
    }

    #[test]
    fn the_worse_of_two_grades_is_the_one_that_counts() {
        assert_eq!(
            VocalGrade::Perfect.worse_of(VocalGrade::Weak),
            VocalGrade::Weak
        );
        assert_eq!(
            VocalGrade::Miss.worse_of(VocalGrade::Perfect),
            VocalGrade::Miss
        );
        assert!(VocalGrade::Weak.hit());
        assert!(!VocalGrade::Miss.hit());
        assert_eq!(VocalGrade::Great.label(), "GREAT");
    }
}
