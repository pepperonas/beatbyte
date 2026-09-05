//! From lyric text as it arrives to the letters the aligner needs.
//!
//! Lyrics come as an `.lrc` (line stamps, maybe word stamps), as
//! plain text, or copied from somewhere with `[Chorus]` markers and
//! `(x2)` repeats in them. The aligner wants a sequence of words,
//! each a run of the model's letters, grouped by line so the result
//! can be shown as lines again. The text the player SEES is kept as
//! it was — only the token sequence is normalised.
//!
//! Rules, not a language model: they cover the catalogue's shapes,
//! and a word that leaves no letters (a number, an emoji, a bar of
//! "—") keeps its place in the line as an *estimated* word, timed
//! between its neighbours.

/// The model's alphabet: `wav2vec2-base-960h`, 32 tokens. Index is
/// the token id. `<pad>` is the CTC blank, `|` the word boundary.
pub const VOCAB: [&str; 32] = [
    "<pad>", "<s>", "</s>", "<unk>", "|", "E", "T", "A", "O", "N", "I", "H", "S", "R", "D", "L",
    "U", "M", "W", "C", "F", "G", "Y", "P", "B", "V", "K", "'", "X", "J", "Q", "Z",
];
/// The CTC blank.
pub const BLANK: u8 = 0;
/// The word-boundary token.
pub const WORD_BOUNDARY: u8 = 4;

/// The token for an uppercase ASCII letter or an apostrophe.
#[must_use]
pub fn token_of(letter: char) -> Option<u8> {
    let wanted = letter.to_ascii_uppercase();
    VOCAB
        .iter()
        .enumerate()
        .skip(5)
        .find(|(_, t)| t.len() == 1 && t.starts_with(wanted))
        .map(|(i, _)| i as u8)
}

/// One word of the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// As written, for display.
    pub text: String,
    /// The letters the aligner looks for; empty for a word the model
    /// has no letters for.
    pub tokens: Vec<u8>,
}

/// One line of the transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// As written (stamps and markers stripped), for display.
    pub text: String,
    /// Its words, in order.
    pub words: Vec<Word>,
    /// The line stamp the source carried, if it was an `.lrc` — the
    /// master-offset check compares against these.
    pub source_start_s: Option<f64>,
}

/// The whole transcript.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transcript {
    /// Lines with at least one word.
    pub lines: Vec<Line>,
}

impl Transcript {
    /// Parse lyric text in any of the shapes above.
    #[must_use]
    pub fn parse(text: &str) -> Transcript {
        let mut lines = Vec::new();
        for raw in text.lines() {
            let (stamp, body) = split_stamp(raw);
            let body = strip_markers(&body);
            let words: Vec<Word> = body
                .split_whitespace()
                .map(|w| Word {
                    text: w.to_owned(),
                    tokens: tokens_of(w),
                })
                .collect();
            if words.is_empty() {
                continue;
            }
            lines.push(Line {
                text: words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                words,
                source_start_s: stamp,
            });
        }
        Transcript { lines }
    }

    /// The letters of every word, with a word boundary between words
    /// (and lines) — the sequence the aligner is forced through.
    /// Words without letters contribute nothing.
    #[must_use]
    pub fn tokens(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for word in self.lines.iter().flat_map(|l| l.words.iter()) {
            if word.tokens.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(WORD_BOUNDARY);
            }
            out.extend_from_slice(&word.tokens);
        }
        out
    }

    /// How many words carry letters.
    #[must_use]
    pub fn alignable_words(&self) -> usize {
        self.lines
            .iter()
            .flat_map(|l| l.words.iter())
            .filter(|w| !w.tokens.is_empty())
            .count()
    }
}

/// A leading `[mm:ss.xx]` line stamp, parsed and removed; a leading
/// `[key:value]` metadata tag or `[Chorus]`-style marker, removed
/// without a stamp.
fn split_stamp(raw: &str) -> (Option<f64>, String) {
    let mut rest = raw.trim();
    let mut stamp = None;
    while let Some(after) = rest.strip_prefix('[') {
        let Some(close) = after.find(']') else { break };
        let inside = &after[..close];
        if let Some(seconds) = parse_stamp(inside) {
            stamp = stamp.or(Some(seconds));
        }
        // Anything in leading brackets is a stamp, a tag or a marker
        // — never sung.
        rest = after[close + 1..].trim_start();
    }
    (stamp, rest.to_owned())
}

/// `mm:ss.xx` (also `mm:ss` and `mm:ss.xxx`) in seconds.
fn parse_stamp(text: &str) -> Option<f64> {
    let (minutes, seconds) = text.split_once(':')?;
    let minutes: f64 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().parse().ok()?;
    if !(0.0..60.0).contains(&seconds) || !(0.0..1000.0).contains(&minutes) {
        return None;
    }
    Some(minutes * 60.0 + seconds)
}

/// Remove inline `<mm:ss.xx>` word stamps and `(x2)`-style repeat
/// marks; keep everything else, parentheses included — backing
/// vocals are sung.
fn strip_markers(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            // Skip through the matching `>` when it looks like a stamp.
            let mut inside = String::new();
            let mut closed = false;
            for d in chars.by_ref() {
                if d == '>' {
                    closed = true;
                    break;
                }
                inside.push(d);
            }
            if !(closed && parse_stamp(&inside).is_some()) {
                out.push('<');
                out.push_str(&inside);
                if closed {
                    out.push('>');
                }
            }
            continue;
        }
        out.push(c);
    }
    // `(x2)`, `(X 3)`, `x2` as its own word.
    out.split_whitespace()
        .filter(|w| !is_repeat_mark(w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_repeat_mark(word: &str) -> bool {
    let bare = word.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']');
    let mut chars = bare.chars();
    matches!(chars.next(), Some('x' | 'X' | '×'))
        && !bare.is_empty()
        && chars.clone().count() > 0
        && chars.all(|c| c.is_ascii_digit())
}

/// The model's letters in a word: accents folded, apostrophes kept,
/// everything else dropped.
#[must_use]
pub fn tokens_of(word: &str) -> Vec<u8> {
    let mut tokens = Vec::new();
    for c in word.chars() {
        for folded in fold(c) {
            if let Some(token) = token_of(folded) {
                tokens.push(token);
            }
        }
    }
    tokens
}

/// One character as the ASCII letters an English model knows.
fn fold(c: char) -> Vec<char> {
    match c {
        '’' | '‘' | '`' | '´' => vec!['\''],
        'ä' | 'Ä' => vec!['A', 'E'],
        'ö' | 'Ö' => vec!['O', 'E'],
        'ü' | 'Ü' => vec!['U', 'E'],
        'ß' => vec!['S', 'S'],
        'à' | 'á' | 'â' | 'ã' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Å' => vec!['A'],
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => vec!['E'],
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => vec!['I'],
        'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ø' => vec!['O'],
        'ù' | 'ú' | 'û' | 'Ù' | 'Ú' | 'Û' => vec!['U'],
        'ç' | 'Ç' => vec!['C'],
        'ñ' | 'Ñ' => vec!['N'],
        'ý' | 'ÿ' | 'Ý' => vec!['Y'],
        other => vec![other],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alphabet_is_the_models_and_letters_map_to_it() {
        assert_eq!(VOCAB.len(), 32);
        assert_eq!(VOCAB[BLANK as usize], "<pad>");
        assert_eq!(VOCAB[WORD_BOUNDARY as usize], "|");
        assert_eq!(token_of('e'), Some(5));
        assert_eq!(token_of('Z'), Some(31));
        assert_eq!(token_of('\''), Some(27));
        assert_eq!(token_of('7'), None);
        assert_eq!(token_of(' '), None);
    }

    #[test]
    fn stamps_tags_and_markers_are_stripped_but_the_words_stay() {
        let text = "[ti:Song]\n[00:12.34]Ooh, don't you <00:12.34>wanna <00:13.00>take her?\n[Chorus]\n(x2)\n[01:02.5] Maria (ave Maria) x2\n\n";
        let t = Transcript::parse(text);
        assert_eq!(t.lines.len(), 2, "{t:?}");
        assert_eq!(t.lines[0].text, "Ooh, don't you wanna take her?");
        assert!((t.lines[0].source_start_s.unwrap_or(0.0) - 12.34).abs() < 1e-9);
        assert_eq!(t.lines[1].text, "Maria (ave Maria)");
        assert!((t.lines[1].source_start_s.unwrap_or(0.0) - 62.5).abs() < 1e-9);
        // The apostrophe is a letter to this model; the comma is not.
        let dont = &t.lines[0].words[1];
        assert_eq!(dont.text, "don't");
        assert_eq!(dont.tokens, vec![14, 8, 9, 27, 6]);
    }

    #[test]
    fn plain_text_needs_no_stamps() {
        let t = Transcript::parse("Hello world\n\nsecond line");
        assert_eq!(t.lines.len(), 2);
        assert_eq!(t.lines[0].source_start_s, None);
        assert_eq!(t.alignable_words(), 4);
    }

    #[test]
    fn accents_fold_and_numbers_leave_an_estimated_word() {
        let t = Transcript::parse("Café 1999 straße");
        let words = &t.lines[0].words;
        assert_eq!(words[0].tokens, tokens_of("CAFE"));
        assert!(words[1].tokens.is_empty(), "a number has no letters here");
        assert_eq!(words[2].tokens, tokens_of("STRASSE"));
        assert_eq!(t.alignable_words(), 2);
    }

    #[test]
    fn the_forced_sequence_puts_a_boundary_between_words_and_lines() {
        let t = Transcript::parse("ab cd\nef");
        let expected: Vec<u8> = [
            tokens_of("ab"),
            vec![WORD_BOUNDARY],
            tokens_of("cd"),
            vec![WORD_BOUNDARY],
            tokens_of("ef"),
        ]
        .concat();
        assert_eq!(t.tokens(), expected);
        // A letterless word adds no boundary of its own.
        let t2 = Transcript::parse("ab 42 cd");
        assert_eq!(
            t2.tokens(),
            [tokens_of("ab"), vec![WORD_BOUNDARY], tokens_of("cd")].concat()
        );
    }

    #[test]
    fn a_stamp_that_is_not_a_time_is_a_marker() {
        assert_eq!(parse_stamp("Chorus"), None);
        assert_eq!(parse_stamp("00:61.0"), None, "61 seconds is not a time");
        assert!((parse_stamp("03:07.250").unwrap_or(0.0) - 187.25).abs() < 1e-9);
    }
}
