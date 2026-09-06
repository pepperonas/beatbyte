//! How much of a song the acoustic model actually heard.
//!
//! A forced alignment always returns a path. It returns one for a
//! song the model understood word for word, and it returns one for a
//! song whose every frame says "blank" — and the second path is not
//! wrong so much as **empty**: the transcript is placed inside
//! whatever windows it was given, because nothing in the audio
//! prefers one position over another.
//!
//! That is not a hypothetical. Böhse Onkelz' *Mexico* is a loud
//! German rock mix under an English character model: over six
//! seconds of singing the model's greedy output is the single letter
//! `I`. The alignment still came back with 39 placed lines, a shift
//! of −3.34 s and 85 % of lines agreeing on it — and every one of
//! those numbers was an artefact. The anchored pass is centred on the
//! shift the previous pass guessed, so with no acoustic preference
//! the words land where the windows are, the deltas reproduce the
//! guess, and the "consensus" measures nothing but the window. The
//! lyrics then ran three seconds early on screen.
//!
//! So the gate needs a fact the alignment cannot give it: did the
//! model hear anything at all? Both measures here are read off the
//! emissions the aligner already computed — no second pass over the
//! audio, no model call.

use crate::ctc::Emissions;

/// What the model heard, independent of any transcript.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evidence {
    /// Share of frames where the model is more sure it heard a
    /// letter than that it heard nothing (`p(blank) < 0.5`).
    pub voiced_share: f32,
    /// Letters per second in the model's own greedy reading, after
    /// the CTC collapse. This is the sharper of the two: it counts
    /// what the model would WRITE, not merely where it hesitated.
    pub letters_per_s: f32,
}

impl Evidence {
    /// Whether an alignment on this song may be believed against the
    /// source's own stamps.
    ///
    /// Below the floor the alignment is not evidence of anything, so
    /// a disagreement with the source is not evidence either — and
    /// the source's stamps, which a human made, are the better
    /// answer.
    #[must_use]
    pub fn is_legible(self, min_letters_per_s: f32) -> bool {
        self.letters_per_s >= min_letters_per_s
    }
}

/// Measure [`Evidence`] on emissions. Pure — tested.
///
/// `blank` is the blank token's index in the model's vocabulary.
#[must_use]
pub fn measure(emissions: &Emissions, blank: usize, frame_s: f64) -> Evidence {
    if emissions.frames == 0 || emissions.vocab == 0 {
        return Evidence {
            voiced_share: 0.0,
            letters_per_s: 0.0,
        };
    }
    let mut voiced = 0usize;
    let mut letters = 0usize;
    // `usize::MAX` cannot be a token index, so the first frame always
    // counts as a change — which is right: a song may open on a word.
    let mut previous = usize::MAX;
    for index in 0..emissions.frames {
        let frame = emissions.frame(index);
        if f64::from(frame[blank]).exp() < 0.5 {
            voiced += 1;
        }
        let best = frame
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(index, _)| index);
        // The CTC collapse: a repeated symbol is one letter, and the
        // blank is what separates two of the same.
        if best != previous && best != blank {
            letters += 1;
        }
        previous = best;
    }
    #[allow(clippy::cast_precision_loss)] // frame counts are small
    let seconds = emissions.frames as f64 * frame_s;
    #[allow(clippy::cast_precision_loss)]
    Evidence {
        voiced_share: (voiced as f64 / emissions.frames as f64) as f32,
        letters_per_s: if seconds > 0.0 {
            (letters as f64 / seconds) as f32
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{Evidence, measure};
    use crate::ctc::Emissions;

    /// Emissions from a script of token indices, one per frame, each
    /// near-certain.
    fn emissions(vocab: usize, frames: &[usize]) -> Emissions {
        let mut log_probs = vec![-9.0f32; frames.len() * vocab];
        for (index, token) in frames.iter().enumerate() {
            log_probs[index * vocab + token] = -0.01;
        }
        Emissions {
            frames: frames.len(),
            vocab,
            log_probs,
        }
    }

    #[test]
    fn a_silent_song_is_not_legible() {
        // Every frame blank: nothing heard, nothing to believe.
        let e = measure(&emissions(4, &[0; 50]), 0, 0.02);
        assert_eq!(e.voiced_share, 0.0);
        assert_eq!(e.letters_per_s, 0.0);
        assert!(!e.is_legible(1.0));
    }

    #[test]
    fn letters_are_counted_after_the_ctc_collapse() {
        // A B B blank B: three letters, not four — the repeat is one
        // letter, and the blank makes the next B a new one.
        let e = measure(&emissions(4, &[1, 2, 2, 0, 2]), 0, 1.0);
        assert!(
            (e.letters_per_s - 3.0 / 5.0).abs() < 1e-6,
            "got {}",
            e.letters_per_s
        );
        // Four of the five frames are letters.
        assert!((e.voiced_share - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_song_the_model_reads_is_legible() {
        // Alternating letters at 20 ms a frame: 25 letters a second.
        let script: Vec<usize> = (0..100).map(|i| 1 + (i % 3)).collect();
        let e = measure(&emissions(4, &script), 0, 0.02);
        assert!(e.is_legible(1.0), "{e:?}");
        assert!(e.voiced_share > 0.99);
    }

    #[test]
    fn the_floor_is_a_comparison_not_a_constant() {
        // The threshold belongs to the caller: the same measurement
        // is legible under one floor and not under another, and the
        // one in use is a measured number that may move.
        let e = Evidence {
            voiced_share: 0.02,
            letters_per_s: 0.9,
        };
        assert!(e.is_legible(0.5));
        assert!(!e.is_legible(1.0));
    }

    #[test]
    fn nothing_measured_is_nothing_claimed() {
        let empty = Emissions {
            frames: 0,
            vocab: 0,
            log_probs: vec![],
        };
        let e = measure(&empty, 0, 0.02);
        assert!(!e.is_legible(0.0001));
    }
}
