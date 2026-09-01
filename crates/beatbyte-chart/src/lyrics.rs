//! Karaoke lyrics: the LRC import format and the internal model.
//!
//! Lyrics are **untrusted input** exactly like charts: a `.lrc` file
//! found beside a song is parsed under hard caps (size, line count,
//! line length, timestamp range) and anything malformed is simply
//! dropped line by line — a bad lyrics file degrades to fewer lines,
//! never to a crash or an unbounded allocation.
//!
//! Two formats:
//!
//! - **Standard LRC** — `[mm:ss.xx]text` line timing. Repeated
//!   stamps (`[t1][t2]text`) emit one line per stamp. The `[offset:]`
//!   metadata tag shifts every stamp (positive = lyrics appear
//!   sooner, the de-facto LRC convention).
//! - **Enhanced LRC** — inline `<mm:ss.xx>` stamps split a line into
//!   karaoke words with absolute spans.
//!
//! The model keeps absolute per-word spans, which is exactly what a
//! later lyrics editor or automatic aligner needs — neither exists
//! yet, but the data does not stand in their way.

/// The largest lyrics file read, in bytes. Lyrics are text; a
/// megabyte is already thousands of lines.
pub const MAX_LYRICS_FILE_BYTES: u64 = 1024 * 1024;
/// The most lines kept from one file.
pub const MAX_LYRIC_LINES: usize = 2000;
/// The most characters kept from one line's text.
pub const MAX_LYRIC_LINE_CHARS: usize = 200;
/// Timestamps beyond this are treated as garbage (24 hours).
pub const MAX_LYRIC_TIME_S: f64 = 86_400.0;
/// A line with no next line (or a distant one) never lingers longer
/// than this past its start / its last word.
pub const LINE_LINGER_S: f64 = 8.0;

/// A song's lyrics: lines sorted by start time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lyrics {
    /// The lines, sorted by `start`.
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    /// Whether any line carries word-level timing.
    #[must_use]
    pub fn has_word_timing(&self) -> bool {
        self.lines.iter().any(|line| !line.words.is_empty())
    }
}

/// One displayed line.
#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    /// When the line becomes the active one (song seconds).
    pub start: f64,
    /// When it stops being shown.
    pub end: f64,
    /// The whole line's text, for layout.
    pub text: String,
    /// Word spans, in order. Empty = line-level timing only.
    pub words: Vec<LyricWord>,
}

/// One karaoke word (an enhanced-LRC span).
#[derive(Debug, Clone, PartialEq)]
pub struct LyricWord {
    /// The word's text (no surrounding whitespace).
    pub text: String,
    /// When the word starts being sung.
    pub start: f64,
    /// When it is fully sung.
    pub end: f64,
}

/// Parse an LRC document into the model. Pure and total: malformed
/// lines are dropped, never fatal. Lines come back sorted by start
/// with ends resolved against their successors.
#[must_use]
pub fn parse_lrc(text: &str) -> Lyrics {
    let mut offset_s = 0.0_f64;
    // First pass: find the offset tag (it may appear anywhere).
    for line in text.lines() {
        if let Some(value) = tag_value(line, "offset")
            && let Ok(ms) = value.trim().trim_start_matches('+').parse::<f64>()
            && ms.is_finite()
            && ms.abs() <= 60_000.0
        {
            // Positive offset = lyrics appear SOONER.
            offset_s = -ms / 1000.0;
        }
    }
    let mut lines: Vec<LyricLine> = Vec::new();
    for raw in text.lines() {
        if lines.len() >= MAX_LYRIC_LINES {
            break;
        }
        let (stamps, rest) = leading_timestamps(raw);
        if stamps.is_empty() {
            continue; // metadata tag, prose, or garbage
        }
        let body: String = rest.chars().take(MAX_LYRIC_LINE_CHARS).collect();
        let (plain, words) = parse_words(&body, offset_s);
        for (which, stamp) in stamps.iter().enumerate() {
            if lines.len() >= MAX_LYRIC_LINES {
                break;
            }
            let start = clamp_time(stamp + offset_s);
            lines.push(LyricLine {
                start,
                end: start, // resolved below
                text: plain.clone(),
                // Word stamps are absolute times: they belong to the
                // first stamped instance; repeats fall back to line
                // timing.
                words: if which == 0 {
                    words.clone()
                } else {
                    Vec::new()
                },
            });
        }
    }
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    resolve_ends(&mut lines);
    Lyrics { lines }
}

/// The `[name:value]` tag's value, if this line is that tag.
fn tag_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (tag, value) = inner.split_once(':')?;
    tag.trim().eq_ignore_ascii_case(name).then_some(value)
}

/// Every leading `[mm:ss.xx]` stamp, and the rest of the line.
fn leading_timestamps(line: &str) -> (Vec<f64>, &str) {
    let mut stamps = Vec::new();
    let mut rest = line.trim_start();
    while let Some(inner_start) = rest.strip_prefix('[') {
        let Some(close) = inner_start.find(']') else {
            break;
        };
        let Some(seconds) = parse_timestamp(&inner_start[..close]) else {
            break; // a metadata tag, not a time
        };
        stamps.push(seconds);
        rest = &inner_start[close + 1..];
    }
    (stamps, rest)
}

/// `mm:ss`, `mm:ss.x` … `mm:ss.xxx` (minutes may run long).
fn parse_timestamp(text: &str) -> Option<f64> {
    let (minutes, seconds) = text.split_once(':')?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().parse().ok()?;
    if !(0.0..60.0).contains(&seconds) {
        return None;
    }
    let total = f64::from(minutes) * 60.0 + seconds;
    (total <= MAX_LYRIC_TIME_S).then_some(total)
}

/// Split an enhanced-LRC body into its plain text and word spans.
/// A body without `<...>` stamps returns the trimmed text and no
/// words.
fn parse_words(body: &str, offset_s: f64) -> (String, Vec<LyricWord>) {
    if !body.contains('<') {
        return (body.trim().to_owned(), Vec::new());
    }
    let mut words: Vec<LyricWord> = Vec::new();
    let mut plain = String::new();
    let mut cursor = body;
    let mut pending: Option<f64> = None;
    let flush = |text: &str, start: Option<f64>, words: &mut Vec<LyricWord>| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(start) = start {
            words.push(LyricWord {
                text: trimmed.to_owned(),
                start: clamp_time(start + offset_s),
                end: 0.0, // resolved below
            });
        }
    };
    while let Some(open) = cursor.find('<') {
        let (before, after_open) = cursor.split_at(open);
        plain.push_str(before);
        flush(before, pending.take(), &mut words);
        let Some(close) = after_open.find('>') else {
            // An unclosed `<` is text, not a stamp.
            plain.push_str(after_open);
            flush(after_open, pending.take(), &mut words);
            cursor = "";
            break;
        };
        let stamp = &after_open[1..close];
        if let Some(seconds) = parse_timestamp(stamp) {
            pending = Some(seconds);
        }
        cursor = &after_open[close + 1..];
    }
    plain.push_str(cursor);
    flush(cursor, pending.take(), &mut words);
    // Resolve word ends: next word's start; the last one stays open
    // until the line end fills it in (`resolve_ends`).
    let next_starts: Vec<f64> = words.iter().skip(1).map(|word| word.start).collect();
    for (index, word) in words.iter_mut().enumerate() {
        word.end = next_starts
            .get(index)
            .copied()
            .unwrap_or(MAX_LYRIC_TIME_S + LINE_LINGER_S);
    }
    let plain = normalize_spaces(&plain);
    (plain, words)
}

/// Collapse runs of whitespace left behind by removed stamps.
fn normalize_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clamp_time(seconds: f64) -> f64 {
    seconds.clamp(0.0, MAX_LYRIC_TIME_S)
}

/// Line ends: the next line's start, capped so a line never lingers
/// through a long instrumental; word-timed lines stay at least until
/// their last word finishes. The last word of a line closes at the
/// line's end.
fn resolve_ends(lines: &mut [LyricLine]) {
    for index in 0..lines.len() {
        let next_start = lines.get(index + 1).map(|next| next.start);
        let line = &mut lines[index];
        let sung_until = line.words.last().map_or(line.start, |word| word.start);
        let natural = sung_until + LINE_LINGER_S;
        let end = next_start.unwrap_or(natural).min(natural).max(line.start);
        line.end = end;
        if let Some(last) = line.words.last_mut() {
            last.end = last.end.min(end);
        }
        // A word must never end before it starts, whatever the file
        // said.
        for word in &mut line.words {
            word.end = word.end.max(word.start);
        }
    }
}

/// What the display shows at a song position. Pure — the
/// deterministic heart of the karaoke renderer: the same position
/// always produces the same cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LyricCue {
    /// The active line's index, if one covers this position.
    pub active: Option<usize>,
    /// The next line still to come, for the preview row.
    pub upcoming: Option<usize>,
}

/// The cue at `position` seconds.
#[must_use]
pub fn cue_at(lyrics: &Lyrics, position: f64) -> LyricCue {
    let upcoming = lyrics.lines.partition_point(|line| line.start <= position);
    // Only the most recently started line can be active: ends are
    // capped at the successor's start, so earlier lines are over.
    let active = upcoming
        .checked_sub(1)
        .filter(|&index| position < lyrics.lines[index].end);
    LyricCue {
        active,
        upcoming: (upcoming < lyrics.lines.len()).then_some(upcoming),
    }
}

/// How far through its span a word is at `position`: 0 before it
/// starts, 1 after it ends, linear in between. A zero-length span
/// snaps to done the moment it starts.
#[must_use]
pub fn word_progress(word: &LyricWord, position: f64) -> f32 {
    if position < word.start {
        return 0.0;
    }
    let span = word.end - word.start;
    if span <= f64::EPSILON {
        return 1.0;
    }
    (((position - word.start) / span) as f32).clamp(0.0, 1.0)
}

/// Parse an lrclib `/api/get` response body into timed lyrics.
///
/// ⚠️ **The preference is inverted against the source this was ported
/// from.** `inspector-rust` (`core/rust-lib/src/shazam.rs`, its
/// `parse_lrclib_response`) prefers `plainLyrics` and *strips* the
/// timestamps out of `syncedLyrics`, because that app only ever
/// displays the words. BeatByte sings along a clock, so it takes
/// `syncedLyrics` — the very field the other app throws away — and
/// treats a track that carries only `plainLyrics` as having no
/// SYNCED lyrics: honest, and distinct from having none at all.
///
/// `None` for both empty (instrumental), malformed JSON, or synced
/// text that parses to nothing. Pure — tested against synthetic
/// bodies (never real lyrics: those are copyrighted).
#[must_use]
pub fn parse_lrclib_response(body: &str) -> Option<Lyrics> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let synced = value
        .get("syncedLyrics")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if synced.trim().is_empty() {
        return None;
    }
    let lyrics = parse_lrc(synced);
    (!lyrics.lines.is_empty()).then_some(lyrics)
}

/// Whether an lrclib response carries words but no timing — the
/// state that deserves its own message instead of "no lyrics".
/// Pure — tested.
#[must_use]
pub fn has_plain_only(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let field = |name| {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .is_empty()
    };
    field("syncedLyrics") && !field("plainLyrics")
}

/// Read and parse a `.lrc` file under the size cap. `None` when the
/// file is missing, oversized, unreadable, or yields no lines.
#[must_use]
pub fn load_lyrics_file(path: &std::path::Path) -> Option<Lyrics> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_LYRICS_FILE_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let lyrics = parse_lrc(&text);
    (!lyrics.lines.is_empty()).then_some(lyrics)
}

/// The `.lrc` beside a song: `audio.lrc` first (the importer copies
/// it there), then `chart.lrc`.
#[must_use]
pub fn lyrics_beside(audio_path: &std::path::Path, chart_path: &std::path::Path) -> Option<Lyrics> {
    load_lyrics_file(&audio_path.with_extension("lrc"))
        .or_else(|| load_lyrics_file(&chart_path.with_extension("lrc")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_lrc_lines_parse_sorted_with_resolved_ends() {
        let doc = "[ti:Test]\n[00:20.00]second line\n[00:12.30]first line\n";
        let lyrics = parse_lrc(doc);
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].text, "first line");
        assert!((lyrics.lines[0].start - 12.3).abs() < 1e-9);
        // End = next line's start (20.0 lies inside the linger cap).
        assert!((lyrics.lines[0].end - 20.0).abs() < 1e-9);
        assert!(!lyrics.has_word_timing());
        // The last line lingers, but not forever.
        assert!((lyrics.lines[1].end - 28.0).abs() < 1e-9);
    }

    #[test]
    fn enhanced_lrc_words_carry_absolute_spans() {
        let doc = "[00:12.30]<00:12.30>Hello <00:12.75>beautiful <00:13.60>world\n[00:15.00]next\n";
        let lyrics = parse_lrc(doc);
        let line = &lyrics.lines[0];
        assert_eq!(line.text, "Hello beautiful world");
        assert_eq!(line.words.len(), 3);
        assert_eq!(line.words[1].text, "beautiful");
        assert!((line.words[1].start - 12.75).abs() < 1e-9);
        // Word end = next word's start; the last word closes at the
        // line's end (the next line's start here).
        assert!((line.words[1].end - 13.6).abs() < 1e-9);
        assert!((line.words[2].end - 15.0).abs() < 1e-9);
        assert!(lyrics.has_word_timing());
    }

    #[test]
    fn repeated_stamps_emit_copies_with_line_timing() {
        // The repeated line itself carries WORD stamps - only then
        // does this test bite: absolute word times can only belong
        // to one instance (the first); the copies must fall back to
        // line timing instead of inheriting times outside their own
        // window. (The first version used a wordless repeated line
        // and stayed green under the mutation.)
        let doc = "[00:10.00][00:30.00]<00:10.00>la <00:10.50>laa\n[00:12.00]<00:12.00>x\n";
        let lyrics = parse_lrc(doc);
        assert_eq!(lyrics.lines.len(), 3);
        assert_eq!(lyrics.lines[0].text, "la laa");
        assert_eq!(lyrics.lines[0].words.len(), 2, "first instance sings");
        assert_eq!(lyrics.lines[2].text, "la laa");
        assert!(
            lyrics.lines[2].words.is_empty(),
            "the copy falls back to line timing"
        );
    }

    #[test]
    fn the_offset_tag_shifts_lyrics_sooner_when_positive() {
        // The de-facto LRC convention: [offset:+500] makes lyrics
        // appear half a second EARLIER.
        let doc = "[offset:+500]\n[00:10.00]line\n";
        let lyrics = parse_lrc(doc);
        assert!((lyrics.lines[0].start - 9.5).abs() < 1e-9);
    }

    #[test]
    fn garbage_degrades_instead_of_breaking() {
        let doc = "no stamp\n[al:Album]\n[99:99.99]bad seconds\n[00:05.00]good\n[xx:yy]nope\n<
        ";
        let lyrics = parse_lrc(doc);
        assert_eq!(lyrics.lines.len(), 1);
        assert_eq!(lyrics.lines[0].text, "good");
        // Caps hold: a hostile file cannot blow up the model.
        let huge = "[00:01.00]x\n".repeat(MAX_LYRIC_LINES * 2);
        assert_eq!(parse_lrc(&huge).lines.len(), MAX_LYRIC_LINES);
        let long_line = format!("[00:01.00]{}", "y".repeat(10_000));
        assert_eq!(
            parse_lrc(&long_line).lines[0].text.chars().count(),
            MAX_LYRIC_LINE_CHARS
        );
    }

    #[test]
    fn the_cue_is_deterministic_across_the_whole_timeline() {
        // The commission's own acceptance test: the same position
        // must always produce the same display state.
        let doc = "[00:10.00]one\n[00:14.00]two\n[00:40.00]three\n";
        let lyrics = parse_lrc(doc);
        let probe = |t: f64| cue_at(&lyrics, t);
        assert_eq!(
            probe(0.0),
            LyricCue {
                active: None,
                upcoming: Some(0)
            }
        );
        assert_eq!(
            probe(10.0),
            LyricCue {
                active: Some(0),
                upcoming: Some(1)
            }
        );
        assert_eq!(
            probe(13.9),
            LyricCue {
                active: Some(0),
                upcoming: Some(1)
            }
        );
        assert_eq!(
            probe(14.0),
            LyricCue {
                active: Some(1),
                upcoming: Some(2)
            }
        );
        // "two" lingers 8 s, then the display goes empty until
        // "three" - an active line must NOT persist through the gap.
        assert_eq!(
            probe(23.0),
            LyricCue {
                active: None,
                upcoming: Some(2)
            }
        );
        assert_eq!(
            probe(40.0),
            LyricCue {
                active: Some(2),
                upcoming: None
            }
        );
        assert_eq!(
            probe(49.0),
            LyricCue {
                active: None,
                upcoming: None
            }
        );
        // Determinism, literally: twice the same answer.
        assert_eq!(probe(13.37), probe(13.37));
    }

    #[test]
    fn an_lrclib_response_yields_the_synced_field() {
        // Synthetic bodies only - real lyrics are copyrighted and
        // never enter this repository.
        let both = r#"{"plainLyrics":"la la","syncedLyrics":"[00:01.00]la\n[00:02.00]la la"}"#;
        let lyrics = parse_lrclib_response(both).expect("synced wins");
        assert_eq!(lyrics.lines.len(), 2);
        assert!((lyrics.lines[0].start - 1.0).abs() < 1e-9);
        assert!(!has_plain_only(both));

        // Words without timing: a state of its own, not "no lyrics".
        let plain_only = r#"{"plainLyrics":"la la","syncedLyrics":""}"#;
        assert!(parse_lrclib_response(plain_only).is_none());
        assert!(has_plain_only(plain_only));

        // Instrumental / malformed / missing: nothing, and NOT the
        // plain-only state either.
        for body in [
            r#"{"plainLyrics":"","syncedLyrics":""}"#,
            r#"{"plainLyrics":null}"#,
            "not json at all",
        ] {
            assert!(parse_lrclib_response(body).is_none(), "body: {body}");
            assert!(!has_plain_only(body), "body: {body}");
        }
    }

    #[test]
    fn word_progress_is_clamped_and_linear() {
        let word = LyricWord {
            text: "x".to_owned(),
            start: 10.0,
            end: 12.0,
        };
        assert!((word_progress(&word, 9.0) - 0.0).abs() < 1e-6);
        assert!((word_progress(&word, 11.0) - 0.5).abs() < 1e-6);
        assert!((word_progress(&word, 13.0) - 1.0).abs() < 1e-6);
        let degenerate = LyricWord {
            text: "x".to_owned(),
            start: 10.0,
            end: 10.0,
        };
        assert!((word_progress(&degenerate, 10.0) - 1.0).abs() < 1e-6);
    }
}
