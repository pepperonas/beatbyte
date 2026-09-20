//! What a singer is asked to sing, and the arithmetic for judging it.
//!
//! A vocal chart is the melody the game holds the player to: parts
//! (one per voice), phrases (what a scoring unit is), notes, and a
//! sparse pitch contour inside each note so a bend or a slide stays a
//! bend and does not flatten to one number.
//!
//! ## Pitch lives in fractional MIDI, not Hz
//!
//! Hertz is confined to the detector boundary — [`hz_to_midi`] is the
//! only door in. Everything the game stores, compares, shows and
//! scores is **fractional MIDI**, and every tolerance is in **cents**
//! (a hundredth of a semitone). That is not a formatting preference:
//! musical distance is logarithmic, so "20 cents out" means the same
//! thing to a bass and to a soprano, while "3 Hz out" does not.
//!
//! ## Time
//!
//! Note and phrase times are `f64` seconds on the song timeline, as
//! everywhere else in this crate. Contour offsets are `f32` seconds
//! **relative to their note's start**, which keeps their precision
//! where it matters and lets a note move without rewriting its shape.

use serde::{Deserialize, Serialize};

/// Concert A, in Hz — the reference [`hz_to_midi`] is anchored to.
pub const HZ_A4: f32 = 440.0;

/// Concert A, as a MIDI note number.
pub const MIDI_A4: f32 = 69.0;

/// Cents in a semitone; the unit every vocal tolerance is expressed in.
pub const CENTS_PER_SEMITONE: f32 = 100.0;

/// Cents in an octave.
pub const CENTS_PER_OCTAVE: f32 = 1200.0;

/// The lowest MIDI note a vocal chart may ask for (≈16.4 Hz, C0) and
/// the highest (≈7902 Hz, B8). Anything outside is a detector artefact
/// or a corrupt file, not a sung note.
pub const MIDI_RANGE: core::ops::RangeInclusive<f32> = 12.0..=119.0;

/// Convert a frequency to a fractional MIDI note number.
///
/// Returns `None` for anything that is not a positive, finite
/// frequency — silence and detector failures must not become pitch
/// zero, which would read as a very low note rather than as no note.
///
/// ```
/// # use beatbyte_core::vocal::hz_to_midi;
/// assert_eq!(hz_to_midi(440.0), Some(69.0));
/// assert_eq!(hz_to_midi(0.0), None);
/// ```
#[must_use]
pub fn hz_to_midi(hz: f32) -> Option<f32> {
    if !hz.is_finite() || hz <= 0.0 {
        return None;
    }
    Some(MIDI_A4 + 12.0 * (hz / HZ_A4).log2())
}

/// Convert a fractional MIDI note number back to Hz.
#[must_use]
pub fn midi_to_hz(midi: f32) -> f32 {
    HZ_A4 * ((midi - MIDI_A4) / 12.0).exp2()
}

/// Signed distance in cents from `target` to `sung`: positive means
/// the singer is sharp (above the target).
#[must_use]
pub fn cents_between(sung_midi: f32, target_midi: f32) -> f32 {
    (sung_midi - target_midi) * CENTS_PER_SEMITONE
}

/// Fold a cent distance into the nearest octave equivalent, in
/// `[-600, +600]`.
///
/// This is what makes a man singing a woman's part right rather than
/// an octave wrong. Exactly ±600 cents is a tritone — equally far
/// either way — and folds to `-600`; the magnitude, which is all
/// scoring uses, is the same either way.
#[must_use]
pub fn fold_octaves(cents: f32) -> f32 {
    if !cents.is_finite() {
        return cents;
    }
    cents - CENTS_PER_OCTAVE * (cents / CENTS_PER_OCTAVE).round()
}

/// How a sung pitch is compared with its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PitchMode {
    /// Octaves do not matter: the distance is folded to the nearest
    /// equivalent. The default, because a chart carries one melody
    /// and the people at the microphone have different voices.
    #[default]
    OctaveIndependent,
    /// The written octave is the target.
    Strict,
}

impl PitchMode {
    /// The signed error in cents under this mode.
    #[must_use]
    pub fn error_cents(self, sung_midi: f32, target_midi: f32) -> f32 {
        let raw = cents_between(sung_midi, target_midi);
        match self {
            PitchMode::OctaveIndependent => fold_octaves(raw),
            PitchMode::Strict => raw,
        }
    }
}

/// Which voice a part is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocalRole {
    /// The main melody.
    #[default]
    Lead,
    /// A harmony or backing line.
    Backing,
    /// First voice of a duet.
    Duet1,
    /// Second voice of a duet.
    Duet2,
}

/// What kind of vocalisation a note asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocalKind {
    /// A sung note with a pitch to hit.
    #[default]
    Pitched,
    /// Rapped: rhythm and delivery, no stable fundamental.
    Rap,
    /// Spoken.
    Spoken,
}

impl VocalKind {
    /// Whether pitch accuracy is part of this note's score at all.
    ///
    /// A rapped note has no fundamental to be wrong about; scoring it
    /// as a failed pitched note would punish the singer for the
    /// music. Rap and speech are judged on timing and coverage, and
    /// pitch simply leaves their denominator.
    #[must_use]
    pub fn is_pitched(self) -> bool {
        matches!(self, VocalKind::Pitched)
    }
}

/// One point of a note's pitch contour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VocalPitchPoint {
    /// Seconds after the note's start.
    pub offset_s: f32,
    /// The pitch there, fractional MIDI.
    pub midi: f32,
    /// How sure the analyser was, 0..=1.
    pub confidence: f32,
}

/// Which of a phrase's tokens a note belongs to, as a half-open range
/// of token indices.
///
/// A named type rather than [`core::ops::Range`]: it is `Copy`, it
/// validates itself, and its JSON shape is fixed by this crate rather
/// than by whatever `Range` happens to serialise as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRange {
    /// First token index, inclusive.
    pub start: u32,
    /// One past the last, exclusive.
    pub end: u32,
}

impl TokenRange {
    /// A range covering exactly one token.
    #[must_use]
    pub fn single(index: u32) -> TokenRange {
        TokenRange {
            start: index,
            end: index.saturating_add(1),
        }
    }

    /// How many tokens it covers.
    #[must_use]
    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Whether it covers nothing.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

/// One word (or syllable, once a language-aware splitter exists) of a
/// phrase, with the span the aligner placed it at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalToken {
    /// As written, for display.
    pub text: String,
    /// Song seconds where it begins.
    pub start_s: f64,
    /// Song seconds where it ends.
    pub end_s: f64,
    /// The aligner's confidence, 0..=1.
    pub confidence: f32,
    /// Index of the word in the song's alignment this came from, when
    /// one is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_word: Option<u32>,
}

/// One note of a vocal part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalNote {
    /// Song seconds where it begins.
    pub start_s: f64,
    /// Song seconds where it ends.
    pub end_s: f64,
    /// What kind of vocalisation it asks for.
    #[serde(default)]
    pub kind: VocalKind,
    /// The nominal pitch, fractional MIDI. `None` for unpitched
    /// notes; for a pitched note it is the contour's centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_midi: Option<f32>,
    /// The pitch over the note's length, sparse and sorted by
    /// [`VocalPitchPoint::offset_s`]. Empty means "flat at
    /// [`VocalNote::target_midi`]".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contour: Vec<VocalPitchPoint>,
    /// How sure the analyser was that this note is real, 0..=1.
    pub confidence: f32,
    /// The tokens this note is sung on, when lyrics were aligned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_range: Option<TokenRange>,
}

impl VocalNote {
    /// The note's length in seconds.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        (self.end_s - self.start_s).max(0.0)
    }

    /// Whether `song_time_s` falls inside `[start_s, end_s)`.
    #[must_use]
    pub fn contains(&self, song_time_s: f64) -> bool {
        song_time_s >= self.start_s && song_time_s < self.end_s
    }

    /// The pitch the note asks for at `song_time_s`, interpolating the
    /// contour linearly between its points.
    ///
    /// Outside the contour's own span the nearest end point holds —
    /// the target never runs off to infinity, and a one-point contour
    /// is simply a flat note. `None` for an unpitched note, or a
    /// pitched one that carries neither contour nor nominal pitch.
    #[must_use]
    pub fn target_at(&self, song_time_s: f64) -> Option<f32> {
        if !self.kind.is_pitched() {
            return None;
        }
        if self.contour.is_empty() {
            return self.target_midi;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "an offset into one note; f32 seconds is the stored precision"
        )]
        let offset = (song_time_s - self.start_s) as f32;
        let first = self.contour.first()?;
        if offset <= first.offset_s {
            return Some(first.midi);
        }
        let last = self.contour.last()?;
        if offset >= last.offset_s {
            return Some(last.midi);
        }
        // The points are sorted, so the first one past `offset` and
        // its predecessor bracket it.
        let upper = self.contour.partition_point(|p| p.offset_s <= offset);
        let before = self.contour.get(upper - 1)?;
        let after = self.contour.get(upper)?;
        let span = after.offset_s - before.offset_s;
        if span <= 0.0 {
            return Some(before.midi);
        }
        let t = (offset - before.offset_s) / span;
        Some(before.midi + (after.midi - before.midi) * t)
    }

    /// The lowest and highest pitch the note asks for.
    #[must_use]
    pub fn pitch_span(&self) -> Option<VocalRange> {
        if !self.kind.is_pitched() {
            return None;
        }
        if self.contour.is_empty() {
            let midi = self.target_midi?;
            return Some(VocalRange {
                low_midi: midi,
                high_midi: midi,
            });
        }
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for point in &self.contour {
            low = low.min(point.midi);
            high = high.max(point.midi);
        }
        (low.is_finite() && high.is_finite()).then_some(VocalRange {
            low_midi: low,
            high_midi: high,
        })
    }
}

/// One phrase: the unit a vocal score is awarded for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalPhrase {
    /// Song seconds where it begins.
    pub start_s: f64,
    /// Song seconds where it ends.
    pub end_s: f64,
    /// How sure the analyser was of the phrase as a whole, 0..=1.
    pub confidence: f32,
    /// Its words, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<VocalToken>,
    /// Its notes, sorted by [`VocalNote::start_s`] and not
    /// overlapping. A gap between two notes is a rest.
    pub notes: Vec<VocalNote>,
}

impl VocalPhrase {
    /// The phrase's length in seconds.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        (self.end_s - self.start_s).max(0.0)
    }

    /// Whether `song_time_s` falls inside `[start_s, end_s)`.
    #[must_use]
    pub fn contains(&self, song_time_s: f64) -> bool {
        song_time_s >= self.start_s && song_time_s < self.end_s
    }

    /// The note sounding at `song_time_s`, if any. `None` in a rest.
    #[must_use]
    pub fn note_at(&self, song_time_s: f64) -> Option<&VocalNote> {
        let upper = self.notes.partition_point(|n| n.start_s <= song_time_s);
        let note = self.notes.get(upper.checked_sub(1)?)?;
        note.contains(song_time_s).then_some(note)
    }

    /// How much of the phrase is covered by notes, in seconds — the
    /// duration a singer is actually asked to produce sound for.
    #[must_use]
    pub fn sung_duration_s(&self) -> f64 {
        self.notes.iter().map(VocalNote::duration_s).sum()
    }
}

/// The lowest and highest pitch of something, in fractional MIDI.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VocalRange {
    /// The lowest pitch.
    pub low_midi: f32,
    /// The highest.
    pub high_midi: f32,
}

impl VocalRange {
    /// A range holding a single pitch.
    #[must_use]
    pub fn at(midi: f32) -> VocalRange {
        VocalRange {
            low_midi: midi,
            high_midi: midi,
        }
    }

    /// The range widened to hold `midi` as well.
    #[must_use]
    pub fn including(self, midi: f32) -> VocalRange {
        VocalRange {
            low_midi: self.low_midi.min(midi),
            high_midi: self.high_midi.max(midi),
        }
    }

    /// The smallest range holding both.
    #[must_use]
    pub fn union(self, other: VocalRange) -> VocalRange {
        VocalRange {
            low_midi: self.low_midi.min(other.low_midi),
            high_midi: self.high_midi.max(other.high_midi),
        }
    }

    /// Its width in semitones.
    #[must_use]
    pub fn semitones(self) -> f32 {
        (self.high_midi - self.low_midi).max(0.0)
    }
}

/// One voice of a song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalPart {
    /// Stable id within the song, e.g. `lead`.
    pub id: String,
    /// Which voice it is.
    #[serde(default)]
    pub role: VocalRole,
    /// A display name, when the song names its singers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its phrases, sorted by [`VocalPhrase::start_s`] and not
    /// overlapping.
    pub phrases: Vec<VocalPhrase>,
}

impl VocalPart {
    /// The phrase active at `song_time_s`, if any.
    #[must_use]
    pub fn phrase_at(&self, song_time_s: f64) -> Option<&VocalPhrase> {
        let upper = self.phrases.partition_point(|p| p.start_s <= song_time_s);
        let phrase = self.phrases.get(upper.checked_sub(1)?)?;
        phrase.contains(song_time_s).then_some(phrase)
    }

    /// Index of the phrase active at `song_time_s`, if any.
    #[must_use]
    pub fn phrase_index_at(&self, song_time_s: f64) -> Option<usize> {
        let upper = self.phrases.partition_point(|p| p.start_s <= song_time_s);
        let index = upper.checked_sub(1)?;
        self.phrases
            .get(index)?
            .contains(song_time_s)
            .then_some(index)
    }

    /// The index of the first phrase that starts at or after
    /// `song_time_s` — what a playhead uses to find the next thing to
    /// sing.
    #[must_use]
    pub fn next_phrase_index(&self, song_time_s: f64) -> usize {
        self.phrases.partition_point(|p| p.start_s < song_time_s)
    }

    /// The note sounding at `song_time_s`, if any.
    #[must_use]
    pub fn note_at(&self, song_time_s: f64) -> Option<&VocalNote> {
        self.phrase_at(song_time_s)?.note_at(song_time_s)
    }

    /// How many notes the part has.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.phrases.iter().map(|p| p.notes.len()).sum()
    }

    /// The pitch range the part asks for, over its pitched notes.
    #[must_use]
    pub fn pitch_range(&self) -> Option<VocalRange> {
        self.phrases
            .iter()
            .flat_map(|p| p.notes.iter())
            .filter_map(VocalNote::pitch_span)
            .reduce(VocalRange::union)
    }

    /// Where the part's singing starts and ends, in song seconds.
    #[must_use]
    pub fn span_s(&self) -> Option<(f64, f64)> {
        let first = self.phrases.first()?;
        let last = self.phrases.last()?;
        Some((first.start_s, last.end_s))
    }
}

/// Everything wrong with a vocal part, in the order it was found.
///
/// Same shape as the chart validator's findings and for the same
/// reason: a bad file should say everything that is bad about it, not
/// stop at the first problem.
#[must_use]
pub fn part_problems(part: &VocalPart, max_song_length_s: f64) -> Vec<String> {
    let mut problems = Vec::new();
    if part.id.is_empty() {
        problems.push("part id is empty".to_owned());
    }
    let ok_time = |t: f64| t.is_finite() && (0.0..=max_song_length_s).contains(&t);
    let mut previous_end = f64::NEG_INFINITY;
    for (pi, phrase) in part.phrases.iter().enumerate() {
        let at = |what: &str| format!("part `{}` phrase[{pi}]: {what}", part.id);
        if !ok_time(phrase.start_s) || !ok_time(phrase.end_s) {
            problems.push(at(&format!(
                "times {:.3}..{:.3} are outside 0..{max_song_length_s:.0} s",
                phrase.start_s, phrase.end_s
            )));
        }
        if phrase.end_s <= phrase.start_s {
            problems.push(at("ends at or before it starts"));
        }
        if phrase.start_s < previous_end {
            problems.push(at("starts before the previous phrase ended"));
        }
        previous_end = phrase.end_s;
        if !(0.0..=1.0).contains(&phrase.confidence) {
            problems.push(at(&format!(
                "confidence {} is outside 0..=1",
                phrase.confidence
            )));
        }
        note_problems(phrase, pi, part, &mut problems, max_song_length_s);
    }
    problems
}

fn note_problems(
    phrase: &VocalPhrase,
    pi: usize,
    part: &VocalPart,
    problems: &mut Vec<String>,
    max_song_length_s: f64,
) {
    let ok_time = |t: f64| t.is_finite() && (0.0..=max_song_length_s).contains(&t);
    let mut previous_end = f64::NEG_INFINITY;
    for (ni, note) in phrase.notes.iter().enumerate() {
        let at = |what: &str| format!("part `{}` phrase[{pi}].notes[{ni}]: {what}", part.id);
        if !ok_time(note.start_s) || !ok_time(note.end_s) {
            problems.push(at(&format!(
                "times {:.3}..{:.3} are outside 0..{max_song_length_s:.0} s",
                note.start_s, note.end_s
            )));
        }
        if note.end_s <= note.start_s {
            problems.push(at("ends at or before it starts"));
        }
        if note.start_s < previous_end {
            problems.push(at("starts before the previous note ended"));
        }
        previous_end = note.end_s;
        if note.start_s < phrase.start_s || note.end_s > phrase.end_s {
            problems.push(at("lies outside its phrase"));
        }
        if !(0.0..=1.0).contains(&note.confidence) {
            problems.push(at(&format!(
                "confidence {} is outside 0..=1",
                note.confidence
            )));
        }
        if let Some(midi) = note.target_midi
            && (!midi.is_finite() || !MIDI_RANGE.contains(&midi))
        {
            problems.push(at(&format!("target {midi} is not a singable pitch")));
        }
        if note.kind.is_pitched() && note.target_midi.is_none() && note.contour.is_empty() {
            problems.push(at("is pitched but carries no pitch"));
        }
        if !note.kind.is_pitched() && note.target_midi.is_some() {
            problems.push(at("is unpitched but carries a target pitch"));
        }
        contour_problems(note, &at, problems);
        if let Some(range) = note.token_range {
            let tokens = u32::try_from(phrase.tokens.len()).unwrap_or(u32::MAX);
            if range.is_empty() || range.end > tokens {
                problems.push(at(&format!(
                    "token range {}..{} is not inside the phrase's {tokens} tokens",
                    range.start, range.end
                )));
            }
        }
    }
}

fn contour_problems(note: &VocalNote, at: &impl Fn(&str) -> String, problems: &mut Vec<String>) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a note's own length; f32 seconds is the contour's stored precision"
    )]
    let duration = note.duration_s() as f32;
    let mut previous = f32::NEG_INFINITY;
    for (ci, point) in note.contour.iter().enumerate() {
        if !point.offset_s.is_finite() || point.offset_s < 0.0 || point.offset_s > duration {
            problems.push(at(&format!(
                "contour[{ci}] offset {} is outside the note",
                point.offset_s
            )));
        }
        if point.offset_s < previous {
            problems.push(at(&format!("contour[{ci}] is out of order")));
        }
        previous = point.offset_s;
        if !point.midi.is_finite() || !MIDI_RANGE.contains(&point.midi) {
            problems.push(at(&format!(
                "contour[{ci}] pitch {} is not a singable pitch",
                point.midi
            )));
        }
        if !(0.0..=1.0).contains(&point.confidence) {
            problems.push(at(&format!(
                "contour[{ci}] confidence {} is outside 0..=1",
                point.confidence
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn phrase(start: f64, end: f64, notes: Vec<VocalNote>) -> VocalPhrase {
        VocalPhrase {
            start_s: start,
            end_s: end,
            confidence: 1.0,
            tokens: Vec::new(),
            notes,
        }
    }

    fn part(phrases: Vec<VocalPhrase>) -> VocalPart {
        VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases,
        }
    }

    #[test]
    fn concert_pitch_is_the_anchor_and_octaves_are_halvings() {
        assert_eq!(hz_to_midi(440.0), Some(69.0));
        let a3 = hz_to_midi(220.0).expect("220 Hz is a pitch");
        assert!((a3 - 57.0).abs() < 1e-4, "220 Hz is A3, got {a3}");
        let a5 = hz_to_midi(880.0).expect("880 Hz is a pitch");
        assert!((a5 - 81.0).abs() < 1e-4, "880 Hz is A5, got {a5}");
        // And back again.
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_hz(57.0) - 220.0).abs() < 1e-3);
    }

    #[test]
    fn silence_is_not_a_very_low_note() {
        // The whole reason this returns an Option: 0 Hz mapped to a
        // number would read as a real, very flat note.
        assert_eq!(hz_to_midi(0.0), None);
        assert_eq!(hz_to_midi(-10.0), None);
        assert_eq!(hz_to_midi(f32::NAN), None);
        assert_eq!(hz_to_midi(f32::INFINITY), None);
    }

    #[test]
    fn a_semitone_is_a_hundred_cents_and_the_sign_says_which_way() {
        assert!(
            (cents_between(70.0, 69.0) - 100.0).abs() < 1e-3,
            "sharp is positive"
        );
        assert!(
            (cents_between(68.0, 69.0) + 100.0).abs() < 1e-3,
            "flat is negative"
        );
        assert_eq!(cents_between(69.0, 69.0), 0.0);
        // A quarter tone, the scale the tolerances live on.
        assert!((cents_between(69.5, 69.0) - 50.0).abs() < 1e-3);
    }

    #[test]
    fn octave_independent_forgives_the_octave_and_strict_does_not() {
        let mode = PitchMode::OctaveIndependent;
        // An octave down, sung perfectly.
        assert!(mode.error_cents(57.0, 69.0).abs() < 1e-3);
        // Two octaves up, also perfect.
        assert!(mode.error_cents(93.0, 69.0).abs() < 1e-3);
        // 20 cents flat an octave down is still 20 cents flat.
        assert!((mode.error_cents(56.8, 69.0) + 20.0).abs() < 1e-2);
        // Strict counts every one of those cents.
        assert!((PitchMode::Strict.error_cents(57.0, 69.0) + 1200.0).abs() < 1e-3);
    }

    #[test]
    fn the_fold_picks_the_nearer_octave_and_is_defined_at_the_tritone() {
        assert!((fold_octaves(1300.0) - 100.0).abs() < 1e-3);
        assert!((fold_octaves(-1300.0) + 100.0).abs() < 1e-3);
        assert!(
            (fold_octaves(700.0) + 500.0).abs() < 1e-3,
            "7 semitones up folds to 5 down"
        );
        // Exactly a tritone is equidistant; it must still be a single
        // defined value, and its magnitude is what scoring reads.
        assert_eq!(fold_octaves(600.0), -600.0);
        assert!(fold_octaves(600.0).abs() - 600.0 < 1e-3);
    }

    #[test]
    fn a_flat_note_holds_its_pitch_and_a_contour_slides_between_points() {
        let flat = note(1.0, 2.0, 60.0);
        assert_eq!(flat.target_at(1.0), Some(60.0));
        assert_eq!(flat.target_at(1.9), Some(60.0));

        let mut bent = note(1.0, 2.0, 60.0);
        bent.contour = vec![
            VocalPitchPoint {
                offset_s: 0.0,
                midi: 60.0,
                confidence: 1.0,
            },
            VocalPitchPoint {
                offset_s: 1.0,
                midi: 62.0,
                confidence: 1.0,
            },
        ];
        let mid = bent.target_at(1.5).expect("a pitched note has a target");
        assert!(
            (mid - 61.0).abs() < 1e-3,
            "halfway through a 2-semitone bend, got {mid}"
        );
        let quarter = bent.target_at(1.25).expect("a pitched note has a target");
        assert!((quarter - 60.5).abs() < 1e-3, "got {quarter}");
    }

    #[test]
    fn the_contour_holds_at_its_ends_instead_of_running_off() {
        let mut bent = note(1.0, 2.0, 60.0);
        bent.contour = vec![
            VocalPitchPoint {
                offset_s: 0.25,
                midi: 60.0,
                confidence: 1.0,
            },
            VocalPitchPoint {
                offset_s: 0.75,
                midi: 64.0,
                confidence: 1.0,
            },
        ];
        // Before the first point and after the last, the nearest end
        // holds — extrapolating a bend would invent pitches the
        // analyser never saw.
        assert_eq!(bent.target_at(1.0), Some(60.0));
        assert_eq!(bent.target_at(1.99), Some(64.0));
    }

    #[test]
    fn an_unpitched_note_never_offers_a_target() {
        let mut rap = note(1.0, 2.0, 60.0);
        rap.kind = VocalKind::Rap;
        assert_eq!(
            rap.target_at(1.5),
            None,
            "rap has no pitch to be wrong about"
        );
        assert_eq!(rap.pitch_span(), None);
        assert!(!VocalKind::Rap.is_pitched());
        assert!(!VocalKind::Spoken.is_pitched());
        assert!(VocalKind::Pitched.is_pitched());
    }

    #[test]
    fn lookup_finds_the_sounding_note_and_nothing_in_a_rest() {
        let p = part(vec![
            phrase(0.0, 4.0, vec![note(0.0, 1.0, 60.0), note(2.0, 3.0, 62.0)]),
            phrase(8.0, 10.0, vec![note(8.0, 9.0, 64.0)]),
        ]);
        assert_eq!(p.note_at(0.5).and_then(|n| n.target_midi), Some(60.0));
        assert_eq!(p.note_at(2.5).and_then(|n| n.target_midi), Some(62.0));
        assert_eq!(p.note_at(8.5).and_then(|n| n.target_midi), Some(64.0));
        // In the rest between the notes, and in the gap between the
        // phrases: nothing is being asked for.
        assert!(p.note_at(1.5).is_none(), "a rest inside a phrase");
        assert!(p.note_at(5.0).is_none(), "the gap between phrases");
        assert!(p.phrase_at(5.0).is_none());
        assert!(p.phrase_at(-1.0).is_none(), "before the song");
        // Half-open: a note's end belongs to what follows it.
        assert!(p.note_at(1.0).is_none());
        assert_eq!(p.phrase_index_at(2.5), Some(0));
        assert_eq!(p.phrase_index_at(9.0), Some(1));
    }

    #[test]
    fn the_playhead_can_find_what_comes_next() {
        let p = part(vec![phrase(0.0, 4.0, vec![]), phrase(8.0, 10.0, vec![])]);
        assert_eq!(p.next_phrase_index(-1.0), 0);
        assert_eq!(p.next_phrase_index(0.0), 0);
        assert_eq!(p.next_phrase_index(1.0), 1);
        assert_eq!(p.next_phrase_index(9.0), 2, "past the last one");
    }

    #[test]
    fn a_part_reports_its_size_and_the_range_it_asks_for() {
        let mut low = note(0.0, 1.0, 55.0);
        low.contour = vec![
            VocalPitchPoint {
                offset_s: 0.0,
                midi: 55.0,
                confidence: 1.0,
            },
            VocalPitchPoint {
                offset_s: 0.5,
                midi: 52.0,
                confidence: 1.0,
            },
        ];
        let p = part(vec![phrase(0.0, 4.0, vec![low, note(2.0, 3.0, 72.0)])]);
        assert_eq!(p.note_count(), 2);
        let range = p.pitch_range().expect("two pitched notes");
        assert!(
            (range.low_midi - 52.0).abs() < 1e-3,
            "the contour's dip counts"
        );
        assert!((range.high_midi - 72.0).abs() < 1e-3);
        assert!((range.semitones() - 20.0).abs() < 1e-3);
        assert_eq!(p.span_s(), Some((0.0, 4.0)));
        // An empty part has neither.
        assert_eq!(part(vec![]).pitch_range(), None);
        assert_eq!(part(vec![]).span_s(), None);
    }

    #[test]
    fn a_range_grows_only_outwards() {
        let r = VocalRange::at(60.0).including(64.0).including(58.0);
        assert_eq!(r.low_midi, 58.0);
        assert_eq!(r.high_midi, 64.0);
        let u = r.union(VocalRange::at(70.0));
        assert_eq!(u.high_midi, 70.0);
        assert_eq!(u.low_midi, 58.0);
    }

    #[test]
    fn a_well_formed_part_has_no_problems() {
        let p = part(vec![
            phrase(0.0, 4.0, vec![note(0.0, 1.0, 60.0), note(2.0, 3.0, 62.0)]),
            phrase(8.0, 10.0, vec![note(8.0, 9.0, 64.0)]),
        ]);
        assert_eq!(part_problems(&p, 600.0), Vec::<String>::new());
    }

    #[test]
    fn validation_catches_every_way_a_part_can_lie() {
        // Overlapping phrases.
        let overlap = part(vec![phrase(0.0, 4.0, vec![]), phrase(3.0, 5.0, vec![])]);
        assert!(
            part_problems(&overlap, 600.0)
                .iter()
                .any(|p| p.contains("before the previous phrase ended")),
            "{:?}",
            part_problems(&overlap, 600.0)
        );

        // A note outside its phrase.
        let escapee = part(vec![phrase(0.0, 2.0, vec![note(0.0, 3.0, 60.0)])]);
        assert!(
            part_problems(&escapee, 600.0)
                .iter()
                .any(|p| p.contains("outside its phrase"))
        );

        // A pitched note with no pitch at all.
        let mut empty = note(0.0, 1.0, 60.0);
        empty.target_midi = None;
        let voiceless = part(vec![phrase(0.0, 2.0, vec![empty])]);
        assert!(
            part_problems(&voiceless, 600.0)
                .iter()
                .any(|p| p.contains("no pitch"))
        );

        // An unpitched note that carries one anyway.
        let mut confused = note(0.0, 1.0, 60.0);
        confused.kind = VocalKind::Spoken;
        let spoken = part(vec![phrase(0.0, 2.0, vec![confused])]);
        assert!(
            part_problems(&spoken, 600.0)
                .iter()
                .any(|p| p.contains("unpitched but carries"))
        );

        // A pitch no human produces.
        let inhuman = part(vec![phrase(0.0, 2.0, vec![note(0.0, 1.0, 200.0)])]);
        assert!(
            part_problems(&inhuman, 600.0)
                .iter()
                .any(|p| p.contains("singable"))
        );

        // Times past the end of the song.
        let late = part(vec![phrase(0.0, 900.0, vec![])]);
        assert!(
            part_problems(&late, 600.0)
                .iter()
                .any(|p| p.contains("outside 0.."))
        );

        // A nameless part.
        let mut nameless = part(vec![]);
        nameless.id = String::new();
        assert!(
            part_problems(&nameless, 600.0)
                .iter()
                .any(|p| p.contains("id is empty"))
        );
    }

    #[test]
    fn a_contour_out_of_order_or_off_the_note_is_a_problem() {
        let mut bad = note(0.0, 1.0, 60.0);
        bad.contour = vec![
            VocalPitchPoint {
                offset_s: 0.8,
                midi: 60.0,
                confidence: 1.0,
            },
            VocalPitchPoint {
                offset_s: 0.2,
                midi: 61.0,
                confidence: 2.0,
            },
            VocalPitchPoint {
                offset_s: 5.0,
                midi: 61.0,
                confidence: 1.0,
            },
        ];
        let problems = part_problems(&part(vec![phrase(0.0, 2.0, vec![bad])]), 600.0);
        assert!(
            problems.iter().any(|p| p.contains("out of order")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("confidence 2")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("outside the note")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_token_range_must_point_at_tokens_that_exist() {
        let mut n = note(0.0, 1.0, 60.0);
        n.token_range = Some(TokenRange { start: 0, end: 3 });
        let mut p = phrase(0.0, 2.0, vec![n]);
        p.tokens = vec![VocalToken {
            text: "hi".to_owned(),
            start_s: 0.0,
            end_s: 1.0,
            confidence: 1.0,
            source_word: None,
        }];
        let problems = part_problems(&part(vec![p]), 600.0);
        assert!(
            problems.iter().any(|x| x.contains("token range")),
            "{problems:?}"
        );

        assert_eq!(TokenRange::single(4), TokenRange { start: 4, end: 5 });
        assert_eq!(TokenRange::single(4).len(), 1);
        assert!(TokenRange { start: 2, end: 2 }.is_empty());
        assert!(!TokenRange::single(0).is_empty());
    }

    #[test]
    fn a_phrase_reports_how_long_it_asks_for_sound() {
        let p = phrase(0.0, 4.0, vec![note(0.0, 1.0, 60.0), note(2.0, 3.5, 62.0)]);
        assert!((p.duration_s() - 4.0).abs() < 1e-9);
        assert!(
            (p.sung_duration_s() - 2.5).abs() < 1e-9,
            "the rest does not count"
        );
    }

    #[test]
    fn the_wire_format_round_trips() {
        let mut n = note(1.0, 2.0, 60.5);
        n.contour = vec![VocalPitchPoint {
            offset_s: 0.5,
            midi: 61.0,
            confidence: 0.9,
        }];
        n.token_range = Some(TokenRange::single(0));
        let mut ph = phrase(1.0, 3.0, vec![n]);
        ph.tokens = vec![VocalToken {
            text: "oh".to_owned(),
            start_s: 1.0,
            end_s: 2.0,
            confidence: 0.8,
            source_word: Some(7),
        }];
        let original = part(vec![ph]);
        let json = serde_json::to_string(&original).expect("serialises");
        let back: VocalPart = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, original);
        // The defaults are omitted, so a minimal file stays minimal.
        assert!(!json.contains("\"name\""), "{json}");
    }

    #[test]
    fn a_minimal_file_loads_with_its_defaults() {
        let json = r#"{
            "id": "lead",
            "phrases": [{
                "start_s": 0.0, "end_s": 1.0, "confidence": 1.0,
                "notes": [{"start_s": 0.0, "end_s": 1.0, "target_midi": 60.0, "confidence": 1.0}]
            }]
        }"#;
        let p: VocalPart = serde_json::from_str(json).expect("the optional fields default");
        assert_eq!(p.role, VocalRole::Lead);
        assert_eq!(p.name, None);
        let n = p.note_at(0.5).expect("one note");
        assert_eq!(n.kind, VocalKind::Pitched);
        assert!(n.contour.is_empty());
        assert_eq!(n.token_range, None);
    }
}
