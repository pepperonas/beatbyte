//! CTC forced alignment: where a KNOWN token sequence sits in a
//! frame-by-frame emission matrix.
//!
//! Not transcription. The transcript is given; the only question is
//! which frame each of its letters occupies, and the answer is the
//! single most probable path through the CTC trellis that spells
//! exactly the transcript — the same Viterbi torchaudio's
//! `forced_align` runs, here in ~150 lines the project owns.
//!
//! The trellis: the sequence is interleaved with blanks
//! (`ø t₁ ø t₂ … tₙ ø`), `2n + 1` states. A frame may stay in its
//! state, advance by one, or advance by two when that skips a blank
//! between two DIFFERENT tokens (a repeated letter needs the blank).
//! Every character of the transcript gets its own frames, which is
//! what the per-glyph fill draws from; nothing is aggregated away
//! here.

use thiserror::Error;

/// Log-probabilities per frame: `frames × vocab`, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Emissions {
    /// Frame count.
    pub frames: usize,
    /// Vocabulary size (the model's).
    pub vocab: usize,
    /// `frames * vocab` log-probabilities, frame-major.
    pub log_probs: Vec<f32>,
}

impl Emissions {
    /// One frame's log-probabilities.
    #[must_use]
    pub fn frame(&self, index: usize) -> &[f32] {
        &self.log_probs[index * self.vocab..(index + 1) * self.vocab]
    }
}

/// Where one token of the sequence landed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenSpan {
    /// The token id.
    pub token: u8,
    /// First frame the token occupies.
    pub start: usize,
    /// One past the last frame it occupies.
    pub end: usize,
    /// The model's confidence in it: the geometric mean of the
    /// token's probability over its frames, 0..1.
    pub score: f32,
}

/// Why an alignment cannot be produced.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AlignError {
    /// Nothing to align.
    #[error("the token sequence is empty")]
    Empty,
    /// The audio is too short for the transcript: every token needs
    /// at least one frame, plus one between two equal tokens.
    #[error("{frames} frames cannot carry {needed} tokens")]
    TooShort {
        /// Frames available.
        frames: usize,
        /// Frames the sequence needs at minimum.
        needed: usize,
    },
    /// A token id outside the model's vocabulary.
    #[error("token {token} is outside a vocabulary of {vocab}")]
    BadToken {
        /// The offending id.
        token: u8,
        /// The vocabulary size.
        vocab: usize,
    },
}

/// The most probable path spelling `tokens` through `emissions`, as
/// one span per token. Pure — tested on synthetic emissions.
pub fn force_align(
    emissions: &Emissions,
    tokens: &[u8],
    blank: u8,
) -> Result<Vec<TokenSpan>, AlignError> {
    if tokens.is_empty() {
        return Err(AlignError::Empty);
    }
    if let Some(&bad) = tokens
        .iter()
        .find(|&&t| usize::from(t) >= emissions.vocab || t == blank)
    {
        return Err(AlignError::BadToken {
            token: bad,
            vocab: emissions.vocab,
        });
    }
    let needed = tokens.len() + tokens.windows(2).filter(|w| w[0] == w[1]).count();
    if emissions.frames < needed {
        return Err(AlignError::TooShort {
            frames: emissions.frames,
            needed,
        });
    }

    // The extended sequence: blank, t1, blank, t2, …, tn, blank.
    let states = 2 * tokens.len() + 1;
    let token_at = |s: usize| -> u8 {
        if s.is_multiple_of(2) {
            blank
        } else {
            tokens[s / 2]
        }
    };
    let frames = emissions.frames;

    // Scores for the previous and current frame; back-pointers for
    // every frame (0 = stayed, 1 = from s-1, 2 = from s-2).
    let mut prev = vec![f32::NEG_INFINITY; states];
    let mut cur = vec![f32::NEG_INFINITY; states];
    let mut back = vec![0u8; frames * states];

    let first = emissions.frame(0);
    prev[0] = first[usize::from(blank)];
    prev[1] = first[usize::from(tokens[0])];

    for t in 1..frames {
        let row = emissions.frame(t);
        // A path must still be able to finish: state s at frame t can
        // only lead to the end if enough frames remain for the tokens
        // after it. Prune the band accordingly — it keeps the sweep
        // linear in practice and never changes the optimum.
        let remaining = frames - t; // frames including this one
        for s in 0..states {
            let emit = row[usize::from(token_at(s))];
            let mut best = prev[s];
            let mut from = 0u8;
            if s >= 1 && prev[s - 1] > best {
                best = prev[s - 1];
                from = 1;
            }
            if s >= 2 && s % 2 == 1 && token_at(s) != token_at(s - 2) && prev[s - 2] > best {
                best = prev[s - 2];
                from = 2;
            }
            // Tokens still to emit after this state, at one frame each.
            let tokens_after = tokens.len() - s.div_ceil(2);
            let value = if best == f32::NEG_INFINITY || tokens_after + 1 > remaining {
                f32::NEG_INFINITY
            } else {
                best + emit
            };
            cur[s] = value;
            back[t * states + s] = from;
        }
        core::mem::swap(&mut prev, &mut cur);
    }

    // End in the last token or the blank after it, whichever scored.
    let last_state = if prev[states - 1] >= prev[states - 2] {
        states - 1
    } else {
        states - 2
    };
    if prev[last_state] == f32::NEG_INFINITY {
        return Err(AlignError::TooShort { frames, needed });
    }

    // Walk back: which state each frame was in.
    let mut path = vec![0usize; frames];
    let mut s = last_state;
    for t in (0..frames).rev() {
        path[t] = s;
        if t > 0 {
            s -= usize::from(back[t * states + s]);
        }
    }

    // Token spans: the maximal run of frames in each token state.
    let mut spans: Vec<TokenSpan> = Vec::with_capacity(tokens.len());
    let mut t = 0;
    while t < frames {
        let state = path[t];
        if state % 2 == 1 {
            let start = t;
            let mut log_sum = 0.0f64;
            while t < frames && path[t] == state {
                log_sum += f64::from(emissions.frame(t)[usize::from(token_at(state))]);
                t += 1;
            }
            let count = (t - start) as f64;
            spans.push(TokenSpan {
                token: token_at(state),
                start,
                end: t,
                score: (log_sum / count).exp().clamp(0.0, 1.0) as f32,
            });
        } else {
            t += 1;
        }
    }
    debug_assert_eq!(spans.len(), tokens.len(), "one span per token");
    Ok(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Emissions where token `t` is near-certain over the frames
    /// `plan` says, and blank everywhere else.
    fn synthetic(vocab: usize, plan: &[(u8, usize)]) -> Emissions {
        let mut log_probs = Vec::new();
        let hot = 0.9f32.ln();
        let cold = ((1.0 - 0.9) / (vocab as f32 - 1.0)).ln();
        for &(token, frames) in plan {
            for _ in 0..frames {
                for v in 0..vocab {
                    log_probs.push(if v == usize::from(token) { hot } else { cold });
                }
            }
        }
        let frames = log_probs.len() / vocab;
        Emissions {
            frames,
            vocab,
            log_probs,
        }
    }

    #[test]
    fn clear_emissions_align_where_they_are() {
        // blank×3, A×4, blank×2, B×3, blank×1: A at frames 3..7, B at 9..12.
        let em = synthetic(4, &[(0, 3), (1, 4), (0, 2), (2, 3), (0, 1)]);
        let spans = force_align(&em, &[1, 2], 0).expect("aligns");
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].token, spans[0].start, spans[0].end), (1, 3, 7));
        assert_eq!((spans[1].token, spans[1].start, spans[1].end), (2, 9, 12));
        assert!(spans[0].score > 0.85 && spans[1].score > 0.85, "{spans:?}");
    }

    #[test]
    fn a_repeated_letter_gets_a_blank_between_and_two_spans() {
        // "AA": A×3, blank×2, A×3. Two separate A spans, in order.
        let em = synthetic(4, &[(1, 3), (0, 2), (1, 3)]);
        let spans = force_align(&em, &[1, 1], 0).expect("aligns");
        assert_eq!((spans[0].start, spans[0].end), (0, 3));
        assert_eq!((spans[1].start, spans[1].end), (5, 8));
        // And when the audio offers NO blank between them — six
        // frames of A for "AA" — the path must still spend a frame
        // in the blank, or the two letters would collapse into one
        // (a mutation that allows the skip produces adjacent spans;
        // this is the case that showed the first pin was blind).
        let em = synthetic(4, &[(1, 6)]);
        let spans = force_align(&em, &[1, 1], 0).expect("aligns");
        assert_eq!(spans.len(), 2);
        assert!(
            spans[1].start > spans[0].end,
            "a blank frame must separate two equal letters: {spans:?}"
        );
    }

    #[test]
    fn the_transcript_wins_over_what_the_model_would_say() {
        // The model is confident of "A B" but we force "A C": C has
        // to be placed somewhere, and it lands where B's frames are,
        // with a LOW score — the forced alignment reports, it does
        // not argue.
        let em = synthetic(4, &[(0, 2), (1, 3), (0, 2), (2, 3), (0, 2)]);
        let spans = force_align(&em, &[1, 3], 0).expect("aligns");
        assert_eq!(spans[1].token, 3);
        assert!(
            spans[1].score < 0.2,
            "a token the model did not hear scores low: {spans:?}"
        );
        assert!(spans[0].score > 0.85);
        assert!(spans[1].start >= spans[0].end, "monotonic");
    }

    #[test]
    fn too_little_audio_and_bad_tokens_are_errors_not_panics() {
        let em = synthetic(4, &[(0, 2)]);
        assert_eq!(
            force_align(&em, &[1, 2, 3], 0),
            Err(AlignError::TooShort {
                frames: 2,
                needed: 3
            })
        );
        // Two equal tokens need three frames (a blank between).
        assert_eq!(
            force_align(&em, &[1, 1], 0),
            Err(AlignError::TooShort {
                frames: 2,
                needed: 3
            })
        );
        assert_eq!(force_align(&em, &[], 0), Err(AlignError::Empty));
        assert_eq!(
            force_align(&em, &[9], 0),
            Err(AlignError::BadToken { token: 9, vocab: 4 })
        );
        assert_eq!(
            force_align(&em, &[0], 0),
            Err(AlignError::BadToken { token: 0, vocab: 4 }),
            "the blank is not a token"
        );
    }

    #[test]
    fn every_token_gets_at_least_one_frame_even_when_the_audio_is_tight() {
        // Exactly as many frames as tokens, all blank-ish: still one
        // span per token, in order, covering the frames.
        let em = synthetic(6, &[(0, 4)]);
        let spans = force_align(&em, &[1, 2, 3, 4], 0).expect("aligns");
        assert_eq!(spans.len(), 4);
        for (i, span) in spans.iter().enumerate() {
            assert_eq!((span.start, span.end), (i, i + 1));
        }
    }

    #[test]
    fn a_long_sequence_aligns_in_reasonable_time() {
        // A song's worth: 12 000 frames (4 min at 50 fps), 1 500 tokens.
        let vocab = 32usize;
        let mut plan = Vec::new();
        let tokens: Vec<u8> = (0..1_500).map(|i| 5 + (i % 26) as u8).collect();
        for &t in &tokens {
            plan.push((0u8, 3usize));
            plan.push((t, 5usize));
        }
        let em = synthetic(vocab, &plan);
        let started = std::time::Instant::now();
        let spans = force_align(&em, &tokens, 0).expect("aligns");
        let took = started.elapsed();
        assert_eq!(spans.len(), tokens.len());
        assert!(took.as_secs_f64() < 20.0, "took {took:?}");
    }
}
