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
//! The model keeps absolute per-word spans, and — from an alignment
//! — per-character spans inside them.
//!
//! - **`<song>.words.json`** — the aligner's output (schema
//!   `beatbyte.lyrics/1`, written by `beatbyte-lyrics`, which this
//!   crate does not depend on: the schema is mirrored here as plain
//!   serde structs and read under the same caps as an `.lrc`). It
//!   wins over an `.lrc` beside the song: it was computed against
//!   this very audio.
//!
//! A per-song **lyric offset** lives beside the audio too
//! (`<song>.lyrics-offset.json`): sources vary per song, and it must
//! survive a realignment, so it is neither in the alignment nor in
//! the settings.

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

/// One karaoke word (an enhanced-LRC span, or an aligned word).
#[derive(Debug, Clone, PartialEq)]
pub struct LyricWord {
    /// The word's text (no surrounding whitespace).
    pub text: String,
    /// When the word starts being sung.
    pub start: f64,
    /// When it is fully sung.
    pub end: f64,
    /// Per-character spans `[start, end]`, one per `char` of `text`,
    /// when an alignment provided them. Empty = the fill runs
    /// linearly across the word.
    pub chars: Vec<[f64; 2]>,
}

impl LyricWord {
    /// A word with linear fill (no character spans).
    #[must_use]
    pub fn new(text: &str, start: f64, end: f64) -> LyricWord {
        LyricWord {
            text: text.to_owned(),
            start,
            end,
            chars: Vec::new(),
        }
    }
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
        if is_instrumental_marker(&plain) {
            continue; // a gap, and the countdown's business
        }
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
            words.push(LyricWord::new(
                trimmed,
                clamp_time(start + offset_s),
                0.0, // resolved below
            ));
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

/// A line that is not sung: empty, or nothing but `♪`, `♫`, dashes,
/// dots — the markers lyric sources put on instrumental passages.
/// Such a line is a GAP to the display (the countdown's business),
/// not a line to show; the font has no glyph for the note anyway.
/// Pure — tested.
#[must_use]
pub fn is_instrumental_marker(text: &str) -> bool {
    !text.chars().any(char::is_alphanumeric)
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
        // said — and its characters stay inside it.
        for word in &mut line.words {
            word.end = word.end.max(word.start);
            for span in &mut word.chars {
                span[0] = span[0].clamp(word.start, word.end);
                span[1] = span[1].clamp(span[0], word.end);
            }
        }
    }
}

/// The largest `words.json` read, in bytes: a four-minute song with
/// character spans is ~150 KB; four megabytes is thousands of lines.
pub const MAX_WORDS_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// The schema this reader understands.
pub const WORDS_SCHEMA: &str = "beatbyte.lyrics/1";
/// The per-song lyric offset is clamped to this many milliseconds
/// either way.
pub const MAX_SONG_LYRIC_OFFSET_MS: i32 = 2000;

/// `words.json` as it is on disk — a mirror of `beatbyte-lyrics`'s
/// schema, deserialised leniently: unknown fields are ignored,
/// missing optional ones default, so a newer writer never breaks an
/// older reader.
#[derive(serde::Deserialize)]
struct WordsFile {
    schema: String,
    #[serde(default)]
    offset_ms: i32,
    #[serde(default)]
    lines: Vec<WordsLine>,
}

#[derive(serde::Deserialize)]
struct WordsLine {
    start: f64,
    #[serde(default)]
    end: f64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    words: Vec<WordsWord>,
}

#[derive(serde::Deserialize)]
struct WordsWord {
    text: String,
    start: f64,
    end: f64,
    #[serde(default)]
    estimated: bool,
    #[serde(default)]
    chars: Vec<[f64; 2]>,
}

/// Parse a `words.json` document. Pure and total under the same
/// discipline as [`parse_lrc`]: a bad line or word is dropped, a bad
/// span is straightened, nothing panics. `None` when the document is
/// not this schema or yields no lines.
#[must_use]
pub fn parse_words_json(text: &str) -> Option<Lyrics> {
    let file: WordsFile = serde_json::from_str(text).ok()?;
    if file.schema != WORDS_SCHEMA {
        return None;
    }
    let offset_s = f64::from(file.offset_ms.clamp(-60_000, 60_000)) / 1000.0;
    let time = |t: f64| t.is_finite().then(|| clamp_time(t + offset_s));
    let mut lines: Vec<LyricLine> = Vec::new();
    for raw in file.lines.into_iter().take(MAX_LYRIC_LINES) {
        let Some(start) = time(raw.start) else {
            continue;
        };
        let text: String = normalize_spaces(&raw.text)
            .chars()
            .take(MAX_LYRIC_LINE_CHARS)
            .collect();
        if is_instrumental_marker(&text) {
            continue;
        }
        let mut words: Vec<LyricWord> = Vec::new();
        let mut any_aligned = false;
        for word in raw.words.into_iter().take(MAX_LYRIC_LINE_CHARS) {
            let (Some(ws), Some(we)) = (time(word.start), time(word.end)) else {
                continue;
            };
            let word_text = normalize_spaces(&word.text);
            if word_text.is_empty() {
                continue;
            }
            // Character spans count only when there is exactly one
            // per character and every one is a finite time; anything
            // else falls back to the linear fill.
            let chars: Vec<[f64; 2]> = if word.chars.len() == word_text.chars().count()
                && word
                    .chars
                    .iter()
                    .all(|c| c[0].is_finite() && c[1].is_finite())
            {
                word.chars
                    .iter()
                    .map(|c| [clamp_time(c[0] + offset_s), clamp_time(c[1] + offset_s)])
                    .collect()
            } else {
                Vec::new()
            };
            any_aligned |= !word.estimated;
            words.push(LyricWord {
                text: word_text,
                start: ws,
                end: we.max(ws),
                chars,
            });
        }
        // A line whose every word is estimated is a line the gate
        // fell back to line level: its word times are an even spread,
        // not knowledge. It is shown as a line-timed line — fade in,
        // hold, fade out — never as a fill that pretends.
        if !any_aligned {
            words.clear();
        }
        words.sort_by(|a, b| a.start.total_cmp(&b.start));
        let end = time(raw.end).unwrap_or(start).max(start);
        lines.push(LyricLine {
            start,
            end,
            text,
            words,
        });
    }
    if lines.is_empty() {
        return None;
    }
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    resolve_ends(&mut lines);
    Some(Lyrics { lines })
}

/// Read and parse a `words.json` under its size cap. `None` when the
/// file is missing, oversized, unreadable, not the schema, or empty.
#[must_use]
pub fn load_words_file(path: &std::path::Path) -> Option<Lyrics> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_WORDS_FILE_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    parse_words_json(&text)
}

/// Where a song's alignment lives: `<audio stem>.words.json` beside
/// the audio.
#[must_use]
pub fn words_path(audio_path: &std::path::Path) -> std::path::PathBuf {
    audio_path.with_extension("words.json")
}

/// Where a song's own lyric offset lives:
/// `<audio stem>.lyrics-offset.json` beside the audio.
#[must_use]
pub fn song_offset_path(audio_path: &std::path::Path) -> std::path::PathBuf {
    audio_path.with_extension("lyrics-offset.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SongOffsetFile {
    offset_ms: i32,
}

/// The song's own lyric offset in milliseconds (positive = lyrics
/// later), 0 when none was saved. Clamped to
/// [`MAX_SONG_LYRIC_OFFSET_MS`].
#[must_use]
pub fn load_song_lyric_offset(audio_path: &std::path::Path) -> i32 {
    let path = song_offset_path(audio_path);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return 0;
    };
    if metadata.len() > 4096 {
        return 0;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<SongOffsetFile>(&text).ok())
        .map_or(0, |file| {
            file.offset_ms
                .clamp(-MAX_SONG_LYRIC_OFFSET_MS, MAX_SONG_LYRIC_OFFSET_MS)
        })
}

/// Persist the song's own lyric offset beside the audio; an offset of
/// 0 removes the file instead (nothing to keep).
pub fn save_song_lyric_offset(audio_path: &std::path::Path, offset_ms: i32) -> std::io::Result<()> {
    let path = song_offset_path(audio_path);
    let offset_ms = offset_ms.clamp(-MAX_SONG_LYRIC_OFFSET_MS, MAX_SONG_LYRIC_OFFSET_MS);
    if offset_ms == 0 {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    let json = serde_json::to_string_pretty(&SongOffsetFile { offset_ms })
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    std::fs::write(path, json)
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

/// The lyrics beside a song: the alignment `audio.words.json` first
/// (computed against this very audio), then `audio.lrc` (the
/// importer copies it there), then `chart.lrc`.
#[must_use]
pub fn lyrics_beside(audio_path: &std::path::Path, chart_path: &std::path::Path) -> Option<Lyrics> {
    load_words_file(&words_path(audio_path))
        .or_else(|| load_lyrics_file(&audio_path.with_extension("lrc")))
        .or_else(|| load_lyrics_file(&chart_path.with_extension("lrc")))
}

/// Whether any lyrics file sits beside a song — the scan's cheap
/// question, the same three places [`lyrics_beside`] reads.
#[must_use]
pub fn lyrics_exist_beside(audio_path: &std::path::Path, chart_path: &std::path::Path) -> bool {
    words_path(audio_path).is_file()
        || audio_path.with_extension("lrc").is_file()
        || chart_path.with_extension("lrc").is_file()
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
        let word = LyricWord::new("x", 10.0, 12.0);
        assert!((word_progress(&word, 9.0) - 0.0).abs() < 1e-6);
        assert!((word_progress(&word, 11.0) - 0.5).abs() < 1e-6);
        assert!((word_progress(&word, 13.0) - 1.0).abs() < 1e-6);
        let degenerate = LyricWord::new("x", 10.0, 10.0);
        assert!((word_progress(&degenerate, 10.0) - 1.0).abs() < 1e-6);
    }

    const WORDS: &str = r#"{
      "schema": "beatbyte.lyrics/1", "audio_sha256": "00", "pipeline_version": 1,
      "language": "en", "source": {"text": "t", "separator": "none", "aligner": "a"},
      "offset_ms": 0,
      "gate": {"verdict": "same_master", "lines_compared": 2},
      "lines": [
        {"start": 20.0, "end": 21.0, "text": "second", "words": [
          {"text": "second", "start": 20.0, "end": 21.0, "conf": 0.2,
           "chars": [[20.0,20.2],[20.2,20.4],[20.4,20.5],[20.5,20.7],[20.7,20.9],[20.9,21.0]]}]},
        {"start": 10.0, "end": 11.5, "text": "Hi there", "words": [
          {"text": "Hi", "start": 10.0, "end": 10.4, "conf": 0.5, "chars": [[10.0,10.2],[10.2,10.4]]},
          {"text": "there", "start": 10.6, "end": 11.5, "conf": 0.0, "estimated": true}]}
      ]
    }"#;

    #[test]
    fn words_json_parses_sorted_with_character_spans_and_real_ends() {
        let lyrics = parse_words_json(WORDS).expect("parses");
        assert_eq!(lyrics.lines.len(), 2);
        let first = &lyrics.lines[0];
        assert_eq!(first.text, "Hi there");
        assert!((first.start - 10.0).abs() < 1e-9);
        // The line's end is its last word's end, not the next line's
        // start: a real end, the renderer dims after it.
        assert!((first.words[1].end - 11.5).abs() < 1e-9);
        assert!(first.end >= 11.5 && first.end <= 20.0);
        assert_eq!(first.words[0].chars.len(), 2, "aligned characters kept");
        assert!(
            first.words[1].chars.is_empty(),
            "an estimated word fills linearly"
        );
        assert_eq!(lyrics.lines[1].words[0].chars.len(), 6);
        assert!(lyrics.has_word_timing());
    }

    #[test]
    fn words_json_is_untrusted_input() {
        // Wrong schema: not ours.
        assert!(parse_words_json(&WORDS.replace("beatbyte.lyrics/1", "other/9")).is_none());
        // Garbage: none, never a panic.
        assert!(parse_words_json("{").is_none());
        assert!(parse_words_json(r#"{"schema":"beatbyte.lyrics/1","lines":[]}"#).is_none());
        // A number JSON cannot hold (1e999) fails the whole document
        // in serde_json - garbage, not lyrics.
        assert!(parse_words_json(&WORDS.replace("20.0,", "1e999,")).is_none());
        // An empty line text drops the line; a mismatched char count
        // drops the spans (linear fill), not the word; a negative
        // time clamps to 0 rather than going anywhere else.
        let doc = r#"{"schema":"beatbyte.lyrics/1","lines":[
          {"start": 3.0, "text": "   ", "words": []},
          {"start": -5.0, "end": 6.0, "text": "ab cd", "words": [
            {"text": "ab", "start": -5.0, "end": 5.5, "chars": [[5.0, 5.2]]},
            {"text": "cd", "start": 5.5, "end": 6.0, "chars": [[5.5, 5.7], [5.7, 5.8], [5.8, 6.0]]}]}]}"#;
        let lyrics = parse_words_json(doc).expect("the good line survives");
        assert_eq!(lyrics.lines.len(), 1);
        assert_eq!(lyrics.lines[0].words.len(), 2);
        assert!(lyrics.lines[0].words.iter().all(|w| w.chars.is_empty()));
        assert!(
            (lyrics.lines[0].start - 0.0).abs() < 1e-9,
            "clamped, not negative"
        );
        // A backwards word is straightened, a char span outside its
        // word is pulled inside.
        let doc = r#"{"schema":"beatbyte.lyrics/1","lines":[
          {"start": 5.0, "end": 6.0, "text": "ab", "words": [
            {"text": "ab", "start": 5.5, "end": 5.0, "chars": [[4.0, 9.0], [5.2, 5.4]]}]}]}"#;
        let word = &parse_words_json(doc).expect("parses").lines[0].words[0];
        assert!(word.end >= word.start);
        assert!(word.chars[0][0] >= word.start && word.chars[0][1] <= word.end);
        // Line and char caps hold.
        let many = format!(
            r#"{{"schema":"beatbyte.lyrics/1","lines":[{}]}}"#,
            (0..MAX_LYRIC_LINES + 50)
                .map(|i| format!(r#"{{"start": {i}.0, "text": "x"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            parse_words_json(&many).expect("parses").lines.len(),
            MAX_LYRIC_LINES
        );
    }

    #[test]
    fn a_line_of_only_estimated_words_is_shown_line_timed() {
        // The gate's line-level fallback spreads words evenly and
        // marks them all estimated; the display must not fill them
        // as if it knew.
        let doc = r#"{"schema":"beatbyte.lyrics/1","lines":[
          {"start": 10.0, "end": 12.0, "text": "ab cd", "words": [
            {"text": "ab", "start": 10.0, "end": 11.0, "estimated": true},
            {"text": "cd", "start": 11.0, "end": 12.0, "estimated": true}]},
          {"start": 20.0, "end": 21.0, "text": "ef gh", "words": [
            {"text": "ef", "start": 20.0, "end": 20.5},
            {"text": "gh", "start": 20.6, "end": 21.0, "estimated": true}]}]}"#;
        let lyrics = parse_words_json(doc).expect("parses");
        assert!(lyrics.lines[0].words.is_empty(), "line-timed");
        assert_eq!(
            lyrics.lines[1].words.len(),
            2,
            "one aligned anchor keeps the words"
        );
    }

    #[test]
    fn instrumental_markers_are_gaps_not_lines() {
        assert!(is_instrumental_marker("♪"));
        assert!(is_instrumental_marker("♪ ♫ ..."));
        assert!(is_instrumental_marker("---"));
        assert!(is_instrumental_marker(""));
        assert!(!is_instrumental_marker("Oh"));
        assert!(!is_instrumental_marker("1999"));
        // Both readers drop them: the `.lrc`...
        let lyrics = parse_lrc("[00:10.00]sung\n[00:20.00]♪\n[00:30.00]more\n");
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[1].text, "more");
        // ...and the alignment.
        let doc = r#"{"schema":"beatbyte.lyrics/1","lines":[
          {"start": 10.0, "text": "sung", "words": []},
          {"start": 20.0, "text": "♪", "words": [{"text": "♪", "start": 20.0, "end": 20.5}]},
          {"start": 30.0, "text": "more", "words": []}]}"#;
        let lyrics = parse_words_json(doc).expect("parses");
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[1].text, "more");
    }

    #[test]
    fn the_offset_field_shifts_every_time_in_the_file() {
        let shifted = WORDS.replace("\"offset_ms\": 0", "\"offset_ms\": 500");
        let lyrics = parse_words_json(&shifted).expect("parses");
        assert!((lyrics.lines[0].start - 10.5).abs() < 1e-9);
        assert!((lyrics.lines[0].words[0].chars[0][0] - 10.5).abs() < 1e-9);
    }

    #[test]
    fn the_song_offset_round_trips_and_zero_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("bb-lyric-offset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let audio = dir.join("song.m4a");
        assert_eq!(load_song_lyric_offset(&audio), 0, "nothing saved = 0");
        save_song_lyric_offset(&audio, -120).expect("saves");
        assert_eq!(load_song_lyric_offset(&audio), -120);
        assert!(song_offset_path(&audio).is_file());
        // Out of range is clamped, both on save and on load.
        save_song_lyric_offset(&audio, 99_999).expect("saves");
        assert_eq!(load_song_lyric_offset(&audio), MAX_SONG_LYRIC_OFFSET_MS);
        std::fs::write(song_offset_path(&audio), r#"{"offset_ms": -99999}"#).expect("writes");
        assert_eq!(load_song_lyric_offset(&audio), -MAX_SONG_LYRIC_OFFSET_MS);
        // Zero removes the file rather than leaving a `0` sidecar.
        save_song_lyric_offset(&audio, 0).expect("saves");
        assert!(!song_offset_path(&audio).is_file());
        save_song_lyric_offset(&audio, 0).expect("removing twice is fine");
        // Garbage in the file reads as 0.
        std::fs::write(song_offset_path(&audio), "nope").expect("writes");
        assert_eq!(load_song_lyric_offset(&audio), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_alignment_beside_a_song_wins_over_its_lrc() {
        let dir = std::env::temp_dir().join(format!("bb-lyrics-beside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let audio = dir.join("song.m4a");
        let chart = dir.join("chart.json");
        assert!(!lyrics_exist_beside(&audio, &chart));
        std::fs::write(
            audio.with_extension("lrc"),
            "[00:01.00]from the lrc
",
        )
        .expect("writes");
        assert!(lyrics_exist_beside(&audio, &chart));
        assert_eq!(
            lyrics_beside(&audio, &chart).expect("lrc").lines[0].text,
            "from the lrc"
        );
        std::fs::write(words_path(&audio), WORDS).expect("writes");
        assert_eq!(
            lyrics_beside(&audio, &chart).expect("words").lines[0].text,
            "Hi there",
            "the alignment was computed against this audio"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
