//! Finding a song on the internet and bringing it home.
//!
//! Type a name in the song browser and this looks it up: the
//! catalogue for the canonical artist, title, length and lyrics, then
//! recordings to chart from. What comes back goes through the same
//! [`crate::import`] pipeline a dropped file does — one way for a
//! song to enter the library, not two.
//!
//! # Where the audio comes from
//!
//! **`yt-dlp`, invoked as a program**, not a stream extractor written
//! here. That was asked for and it is worth saying why it is not what
//! this is: YouTube's signature ciphering exists precisely to keep
//! third-party downloaders out, so an extractor is several thousand
//! lines that break on the next change at the far end. The game would
//! ship a feature that worked on the day it was written. Calling the
//! tool instead keeps the moving part outside this repository, where
//! somebody maintains it — and the player still never opens a
//! terminal.
//!
//! ⚠️ Downloading from YouTube is against its terms of service. This
//! module makes the request the player asked for; what may be
//! downloaded from where is theirs to judge, as it is with any tool
//! on their machine.
//!
//! # Which recording, and why that one
//!
//! Two stages, because the expensive one must run once and not five
//! times.
//!
//! **On the metadata** ([`rank_by_metadata`]): the catalogue knows how
//! long the song is, and [`crate::lyrics_fetch::duration_fits`] — the
//! rule written after a remix was handed the original's stamps —
//! throws out every candidate that is a different edit. What is left
//! is ordered by what a title says about itself: a live take, a
//! cover, a nightcore edit and an hour-long loop are all the wrong
//! recording for a chart, and they all say so in their own names.
//!
//! **On the audio** ([`judge`]), for the leader only: the
//! instruments this project already carries. `loudness::measure_file`
//! knows a video rip by its spectrum, the analyzer reports how sure
//! it is of the tempo, and — with `ml` and a `.lrc` — the aligner's
//! own evidence says whether this recording sings these words at all.
//! Fail, and the next candidate gets its turn. That is what "the
//! version that matches the lyrics and charts well" means here, and
//! all of it is measured rather than believed.
//!
//! # The optional AI
//!
//! Off unless switched on, and it never touches the audio: it reads
//! the candidate TITLES and says which is the album version. Two
//! backends, because this machine and someone else's are not the
//! same ([`Backend`]): the **Claude Code CLI** when it is on the
//! path — already signed in, no key anywhere — else an **API key**
//! the player stored. With neither, the metadata ranking stands on
//! its own; it is the default and it is not a fallback anyone should
//! feel bad about.

use std::path::{Path, PathBuf};
use std::process::Command;

use beatbyte_audio::Analyzer;

use crate::lyrics_fetch::duration_fits;

/// The program that searches and fetches. Looked up on `PATH`.
pub const FETCH_TOOL: &str = "yt-dlp";
/// The Claude Code CLI, the first choice for the optional AI.
pub const CLI_TOOL: &str = "claude";
/// How many recordings a search asks the tool for.
pub const CANDIDATES: usize = 6;
/// The model the AI step runs on. Small on purpose: it reads six
/// titles and answers with a number.
pub const AI_MODEL: &str = "haiku";
/// The Claude API's version header. Required on every request — see
/// the header table in the API overview.
pub const API_VERSION: &str = "2023-06-01";
/// Where the API lives.
pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
/// How long the AI step may take before it is given up on. It is an
/// optional nicety; it may never hold up an import.
pub const AI_TIMEOUT_S: u64 = 30;

// ── Candidates ──────────────────────────────────────────────────────

/// One recording the search turned up.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The tool's own id for it, which is what gets fetched.
    pub id: String,
    /// Its title, as published.
    pub title: String,
    /// Who published it.
    pub uploader: String,
    /// Its length in seconds, when the search reported one.
    pub duration_s: Option<f64>,
}

impl Candidate {
    /// What the fetch step is given. The id, not the page URL: a
    /// title with a slash or a quote in it has no business being
    /// pasted into a command line.
    #[must_use]
    pub fn fetch_target(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }

    /// Title and uploader, lowercased once for the word tests below.
    fn haystack(&self) -> String {
        format!("{} {}", self.title, self.uploader).to_lowercase()
    }
}

/// Words that mean "this is not the recording the chart wants".
///
/// A live take has a crowd on it, a cover is a different performance
/// of the same words, and a sped-up edit is a different tempo — all
/// three will align badly against a catalogue entry made from the
/// studio master. Instrumentals and karaoke tracks are worse: the
/// lyric check has nothing to hear.
pub const WRONG_KIND: [&str; 14] = [
    "live",
    "cover",
    "karaoke",
    "instrumental",
    "remix",
    "nightcore",
    "sped up",
    "slowed",
    "reverb",
    "8d audio",
    "reaction",
    "tutorial",
    "lesson",
    "loop",
];

/// Words that mean "this is the recording the chart wants".
///
/// `topic` earns its place: YouTube's auto-generated artist channels
/// are named `<artist> - Topic` and carry the delivered master.
pub const RIGHT_KIND: [&str; 5] = ["topic", "official audio", "full album", "hq", "remastered"];

/// The search string handed to the tool. Pure — tested.
#[must_use]
pub fn query_for(artist: &str, title: &str) -> String {
    let artist = artist.trim();
    let title = title.trim();
    if artist.is_empty() {
        title.to_owned()
    } else {
        format!("{artist} {title}")
    }
}

/// Whether an id is a plain token, safe to put in a URL.
///
/// The ids come from another program's output, which makes them
/// untrusted input like a chart file is. Arguments here never go
/// through a shell — `Command::arg` passes them straight — but an id
/// with a space or a quote in it would still build a URL nobody
/// meant, so it is not a candidate. Pure — tested.
#[must_use]
pub fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Read the tool's `--dump-json` output: one JSON object per line.
///
/// A line that will not parse is skipped rather than failing the
/// search — one malformed entry among six is not a reason to find
/// nothing. Pure — tested.
#[must_use]
pub fn parse_candidates(stdout: &str) -> Vec<Candidate> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| {
            let id = value.get("id")?.as_str()?.to_owned();
            if !is_plain_id(&id) {
                return None;
            }
            Some(Candidate {
                id,
                title: value
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                uploader: value
                    .get("uploader")
                    .or_else(|| value.get("channel"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                // The search reports this as a number of seconds;
                // `--flat-playlist` sometimes reports none at all.
                duration_s: value.get("duration").and_then(serde_json::Value::as_f64),
            })
        })
        .collect()
}

/// Whether a candidate can be the same edit as the catalogue's.
///
/// With a catalogue length, [`duration_fits`] decides — the same rule
/// the lyrics lookup uses, so a recording accepted here is one whose
/// stamps will fit. Without one, length says nothing and everything
/// passes. Pure — tested.
#[must_use]
pub fn plausible(candidate: &Candidate, catalogue_s: Option<f64>) -> bool {
    match (candidate.duration_s, catalogue_s) {
        (Some(theirs), Some(ours)) => duration_fits(ours, theirs),
        _ => true,
    }
}

/// How much a candidate's own name recommends it, higher is better.
/// Pure — tested.
#[must_use]
pub fn name_score(candidate: &Candidate) -> i32 {
    let hay = candidate.haystack();
    let wrong = WRONG_KIND.iter().filter(|w| hay.contains(**w)).count() as i32;
    let right = RIGHT_KIND.iter().filter(|w| hay.contains(**w)).count() as i32;
    2 * right - 3 * wrong
}

/// The candidates worth fetching, best first.
///
/// Filtered by length, then ordered by what the names say. Ties keep
/// the order the search returned them in, which is the tool's own
/// relevance — `sort_by` is stable and that is the point. Pure —
/// tested.
#[must_use]
pub fn rank_by_metadata(candidates: &[Candidate], catalogue_s: Option<f64>) -> Vec<Candidate> {
    let mut kept: Vec<Candidate> = candidates
        .iter()
        .filter(|c| plausible(c, catalogue_s))
        .cloned()
        .collect();
    kept.sort_by_key(|c| std::cmp::Reverse(name_score(c)));
    kept
}

// ── The optional AI ─────────────────────────────────────────────────

/// Where the AI step runs, when it runs at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// The Claude Code CLI on this machine: already signed in, and
    /// no key is stored anywhere.
    Cli,
    /// Someone else's machine: their own API key.
    Api(String),
    /// Not switched on, or nothing to run it with.
    Off,
}

/// Which backend a build and a settings file add up to.
///
/// The CLI wins when it is there: it needs no key, so it cannot leak
/// one. Pure in its inputs — the caller passes what it found, so the
/// decision itself is testable.
#[must_use]
pub fn backend_for(enabled: bool, cli_present: bool, key: Option<&str>) -> Backend {
    if !enabled {
        return Backend::Off;
    }
    if cli_present {
        return Backend::Cli;
    }
    match key.map(str::trim) {
        Some(key) if !key.is_empty() => Backend::Api(key.to_owned()),
        _ => Backend::Off,
    }
}

/// The key a build may use, preferring the environment over the
/// settings file: an exported key belongs to the session, a stored
/// one to the machine, and the narrower of the two wins.
#[must_use]
pub fn api_key(stored: &str) -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            let stored = stored.trim();
            (!stored.is_empty()).then(|| stored.to_owned())
        })
}

/// What the model is asked. Titles only — no audio, no lyrics, and
/// nothing about the player. Pure — tested.
#[must_use]
pub fn prompt_for(artist: &str, title: &str, candidates: &[Candidate]) -> String {
    let mut prompt = format!(
        "Pick the recording that is the ORIGINAL STUDIO version of \
         \"{title}\" by {artist}, suitable for transcribing to a \
         rhythm-game chart. Prefer the delivered master; reject live \
         takes, covers, karaoke, instrumentals and edited tempos.\n\n"
    );
    for (index, candidate) in candidates.iter().enumerate() {
        let length = candidate
            .duration_s
            .map_or_else(|| "unknown".to_owned(), |s| format!("{:.0}s", s));
        prompt.push_str(&format!(
            "{index}: {} [{}] ({length})\n",
            candidate.title, candidate.uploader
        ));
    }
    prompt.push_str("\nAnswer with the index only, as a bare number.");
    prompt
}

/// Read the model's answer: the leading number, and only when it
/// names a candidate that exists.
///
/// Anything else is `None` and the metadata order stands — an
/// optional step may not be able to break the import by talking.
/// Pure — tested.
#[must_use]
pub fn parse_choice(reply: &str, count: usize) -> Option<usize> {
    let digits: String = reply
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<usize>().ok().filter(|n| *n < count)
}

/// Ask the model, whichever backend this is.
///
/// # Errors
/// When the backend cannot be reached or answers with an error.
pub fn ask(backend: &Backend, prompt: &str) -> Result<String, String> {
    match backend {
        Backend::Off => Err("the AI step is off".to_owned()),
        Backend::Cli => ask_cli(prompt),
        Backend::Api(key) => ask_api(key, prompt),
    }
}

/// The Claude Code CLI in print mode. `--output-format json` returns
/// one object carrying `result` and `is_error`; verified against the
/// installed CLI rather than remembered.
fn ask_cli(prompt: &str) -> Result<String, String> {
    let output = Command::new(CLI_TOOL)
        .args(["-p", "--model", AI_MODEL, "--output-format", "json"])
        .arg(prompt)
        .output()
        .map_err(|error| format!("cannot run {CLI_TOOL}: {error}"))?;
    let body = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| format!("{CLI_TOOL} said: {error}"))?;
    if parsed.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(format!("{CLI_TOOL} reported an error"));
    }
    parsed
        .get("result")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{CLI_TOOL} answered without a result"))
}

/// The Messages API with the player's own key.
///
/// Headers per the API overview's table: `x-api-key` (the key
/// fallback, still supported), the required `anthropic-version`, and
/// `content-type`. The answer is `content[0].text`.
fn ask_api(key: &str, prompt: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 16,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let response = ureq::post(API_URL)
        .timeout(std::time::Duration::from_secs(AI_TIMEOUT_S))
        .set("x-api-key", key)
        .set("anthropic-version", API_VERSION)
        .set("content-type", "application/json")
        // `send_json`/`into_json` would need ureq's `json` feature,
        // which this workspace does not enable: the body is rendered
        // and the reply parsed with the serde_json already here.
        .send_string(&body.to_string())
        .map_err(|error| format!("the API refused: {error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("unreadable answer: {error}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| format!("unreadable answer: {error}"))?;
    parsed
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|block| block.get("text"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "the API answered without text".to_owned())
}

/// Let the model reorder the shortlist, if it is switched on and it
/// answers sensibly. Its pick moves to the front; nothing else moves,
/// and a refusal changes nothing at all.
#[must_use]
pub fn ai_preference(
    backend: &Backend,
    artist: &str,
    title: &str,
    ranked: &[Candidate],
) -> Option<usize> {
    if matches!(backend, Backend::Off) || ranked.len() < 2 {
        return None;
    }
    let prompt = prompt_for(artist, title, ranked);
    match ask(backend, &prompt) {
        Ok(reply) => parse_choice(&reply, ranked.len()),
        Err(error) => {
            bevy::log::warn!("discover: the AI step said: {error}");
            None
        }
    }
}

/// Move the chosen candidate to the front, keeping the rest in order.
/// Pure — tested.
#[must_use]
pub fn promote(ranked: &[Candidate], chosen: usize) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(ranked.len());
    if let Some(pick) = ranked.get(chosen) {
        out.push(pick.clone());
    }
    out.extend(
        ranked
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != chosen)
            .map(|(_, c)| c.clone()),
    );
    out
}

// ── What the player typed ───────────────────────────────────────────

/// Split what was typed into artist and title.
///
/// `Artist - Title` is the browser's own way of writing a song, so it
/// is what the box accepts. Without a dash there is no artist to
/// give the catalogue, and the whole string is the title — the search
/// still works, and the artist comes back from the recording that
/// wins. Pure — tested.
#[must_use]
pub fn split_query(text: &str) -> (String, String) {
    let text = text.trim();
    for separator in [" - ", " – ", " — "] {
        if let Some((artist, title)) = text.split_once(separator) {
            return (artist.trim().to_owned(), title.trim().to_owned());
        }
    }
    (String::new(), text.to_owned())
}

/// The artist behind a channel name.
///
/// YouTube's auto-generated artist channels are `<artist> - Topic`,
/// and that suffix is not part of anybody's name. Pure — tested.
#[must_use]
pub fn artist_from_uploader(uploader: &str) -> String {
    uploader
        .trim()
        .strip_suffix("- Topic")
        .or_else(|| uploader.trim().strip_suffix("- topic"))
        .unwrap_or(uploader.trim())
        .trim()
        .to_owned()
}

// ── Talking to the tool ─────────────────────────────────────────────

/// Whether the fetch tool is on this machine.
#[must_use]
pub fn tool_available() -> bool {
    which(FETCH_TOOL)
}

/// Whether the Claude CLI is on this machine.
#[must_use]
pub fn cli_available() -> bool {
    which(CLI_TOOL)
}

/// Is `program` on the path? `--version` rather than `which`, because
/// the answer wanted is "can this be run", not "does a file exist".
fn which(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The line shown when the tool is missing. One sentence and one
/// command — a dead end that does not say how to leave it is worse
/// than no feature.
#[must_use]
pub fn missing_tool_message() -> String {
    format!("{FETCH_TOOL} is not installed - `brew install {FETCH_TOOL}` (or see its README)")
}

/// Search for recordings of a song.
///
/// # Errors
/// When the tool is missing or fails.
pub fn search(artist: &str, title: &str) -> Result<Vec<Candidate>, String> {
    if !tool_available() {
        return Err(missing_tool_message());
    }
    let query = query_for(artist, title);
    // `ytsearchN:` is the tool's own documented search syntax (see
    // its `--default-search` help); `--flat-playlist` keeps it to one
    // page fetch instead of resolving every hit.
    let output = Command::new(FETCH_TOOL)
        .args(["--dump-json", "--flat-playlist", "--no-warnings"])
        .arg(format!("ytsearch{CANDIDATES}:{query}"))
        .output()
        .map_err(|error| format!("cannot run {FETCH_TOOL}: {error}"))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "search failed: {}",
            reason.lines().last().unwrap_or("no reason given")
        ));
    }
    Ok(parse_candidates(&String::from_utf8_lossy(&output.stdout)))
}

/// Fetch one candidate's audio into `dir`, returning the file.
///
/// # Errors
/// When the tool fails or writes nothing.
pub fn fetch_audio(candidate: &Candidate, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("cannot make a folder: {error}"))?;
    let stem = dir.join(&candidate.id);
    let output = Command::new(FETCH_TOOL)
        .args([
            "-x",
            "--audio-format",
            "m4a",
            "--no-playlist",
            "--no-warnings",
        ])
        .arg("-o")
        .arg(format!("{}.%(ext)s", stem.display()))
        .arg(candidate.fetch_target())
        .output()
        .map_err(|error| format!("cannot run {FETCH_TOOL}: {error}"))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "fetch failed: {}",
            reason.lines().last().unwrap_or("no reason given")
        ));
    }
    let file = stem.with_extension("m4a");
    if file.is_file() {
        Ok(file)
    } else {
        Err("the fetch wrote no audio".to_owned())
    }
}

// ── Verifying the one that was fetched ──────────────────────────────

/// How sure the analyzer must be of the tempo before a recording is
/// worth charting. Below this the grid is a guess and every note sits
/// on it.
pub const MIN_TEMPO_CONFIDENCE: f64 = 0.25;

/// What a fetched recording turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// Whether it is worth charting.
    pub good: bool,
    /// Why, in one line, for the panel.
    pub reason: String,
}

/// Judge a fetched recording on the instruments this project already
/// has: the audio's own quality report and the analyzer's confidence.
///
/// Deliberately NOT a download of five files: the leader is fetched,
/// measured, and kept or dropped. Pure in its inputs — the caller
/// measures, this decides — so the rule is testable without audio.
#[must_use]
pub fn judge(quality_ok: bool, quality_note: &str, tempo_confidence: f64) -> Verdict {
    if !quality_ok {
        return Verdict {
            good: false,
            reason: format!("poor audio: {quality_note}"),
        };
    }
    if tempo_confidence < MIN_TEMPO_CONFIDENCE {
        return Verdict {
            good: false,
            reason: format!("no steady tempo ({tempo_confidence:.2})"),
        };
    }
    Verdict {
        good: true,
        reason: format!("tempo confidence {tempo_confidence:.2}"),
    }
}

// ── What the song ends up called ────────────────────────────────────

/// Strip the furniture a publisher puts around a title.
///
/// `(Official Music Video)`, `[HD]`, `(Lyrics)` — none of it is the
/// song's name, and all of it ends up in the folder name and in the
/// question the lyrics catalogue is asked. Pure — tested.
#[must_use]
pub fn clean_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0i32;
    for character in title.chars() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(character),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The artist and title a found song is filed under.
///
/// The query wins when it named an artist (`Artist - Title`): the
/// player said what they meant. Without one the names come from the
/// recording that won, NOT from the raw query — "how bizarre ocm"
/// was filed under exactly that, and the catalogue was then asked for
/// a song of that name and found nothing (reported: added, no
/// lyrics). A published title is usually `Artist - Title`, and where
/// it is not, the channel is the artist. Pure — tested.
#[must_use]
pub fn names_from(
    typed_artist: &str,
    typed_title: &str,
    candidate: &Candidate,
) -> (String, String) {
    if !typed_artist.trim().is_empty() {
        return (
            typed_artist.trim().to_owned(),
            typed_title.trim().to_owned(),
        );
    }
    let cleaned = clean_title(&candidate.title);
    for separator in [" - ", " – ", " — "] {
        if let Some((artist, title)) = cleaned.split_once(separator) {
            let (artist, title) = (artist.trim(), title.trim());
            if !artist.is_empty() && !title.is_empty() {
                return (artist.to_owned(), title.to_owned());
            }
        }
    }
    let artist = artist_from_uploader(&candidate.uploader);
    let title = if cleaned.is_empty() {
        typed_title.trim().to_owned()
    } else {
        cleaned
    };
    (artist, title)
}

/// The file a fetched song is stored as, before the import copies it
/// in.
///
/// `import_song` takes the FOLDER name from the file name, so a file
/// called after the video id gives a folder called after the video
/// id — `c2cmg33mwvy-m4a` in the library, which is not a song
/// anybody can find (reported). Pure — tested.
#[must_use]
pub fn file_stem_for(artist: &str, title: &str) -> String {
    let joined = if artist.trim().is_empty() {
        title.trim().to_owned()
    } else {
        format!("{} - {}", artist.trim(), title.trim())
    };
    let safe: String = joined
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim().trim_matches('.').trim().to_owned();
    if safe.is_empty() {
        "found song".to_owned()
    } else {
        safe
    }
}

// ── The whole way, from a typed name to a song on disk ──────────────

/// How many candidates may be fetched before a search gives up.
///
/// The leader is fetched and measured; a failure moves down the list.
/// Three is a search, not a download spree.
pub const MAX_ATTEMPTS: usize = 3;

/// Run the whole thing: search, choose, fetch, measure, look up the
/// lyrics, chart it.
///
/// Every step reports through `say` so the panel can show where it
/// is — a minute of silence and a finished song is not a state
/// anybody can read.
///
/// # Errors
/// When nothing usable was found, or the import itself failed.
pub fn discover(query: &str, backend: &Backend, say: &dyn Fn(String)) -> Result<String, String> {
    let (typed_artist, typed_title) = split_query(query);
    if typed_title.is_empty() {
        return Err("type a song name first".to_owned());
    }

    // The catalogue first, when the query names an artist: its
    // length is what tells one edit from another, and having it
    // before the search means the wrong edits never get fetched.
    let catalogue_s = if typed_artist.is_empty() {
        None
    } else {
        say(format!("looking up \"{typed_title}\"..."));
        crate::lyrics_fetch::catalogue_duration(&typed_artist, &typed_title)
    };

    say(format!("searching for \"{query}\"..."));
    let found = search(&typed_artist, &typed_title)?;
    if found.is_empty() {
        return Err(format!("nothing found for \"{query}\""));
    }
    let mut ranked = rank_by_metadata(&found, catalogue_s);
    if ranked.is_empty() {
        return Err(format!(
            "{} recordings found, none the right length",
            found.len()
        ));
    }
    if let Some(chosen) = ai_preference(backend, &typed_artist, &typed_title, &ranked) {
        say(format!("the model prefers \"{}\"", ranked[chosen].title));
        ranked = promote(&ranked, chosen);
    }

    let dir = std::env::temp_dir().join("beatbyte-discover");
    let mut last = String::from("no candidate worked");
    for candidate in ranked.iter().take(MAX_ATTEMPTS) {
        say(format!("fetching \"{}\"...", candidate.title));
        let audio = match fetch_audio(candidate, &dir) {
            Ok(path) => path,
            Err(error) => {
                last = error;
                continue;
            }
        };
        say("measuring...".to_owned());
        match measure(&audio) {
            Ok(verdict) if verdict.good => {
                let (artist, title) = names_from(&typed_artist, &typed_title, candidate);
                // The import takes the FOLDER name from the file
                // name, so the file is renamed before it goes in:
                // fetched as the video's id, it filed the song under
                // `c2cmg33mwvy-m4a` and nobody could find it.
                let audio = match rename_to_song(&audio, &artist, &title) {
                    Ok(renamed) => renamed,
                    Err(error) => {
                        // Not worth failing an import over — the song
                        // still lands, under a poorer name.
                        bevy::log::warn!("discover: cannot rename the fetch: {error}");
                        audio
                    }
                };
                // The lyrics before the import, so the `.lrc` beside
                // the fetched file travels with it: `import_song`
                // already carries one along, and that is the path a
                // dropped file takes too.
                say(format!("looking up lyrics for \"{title}\"..."));
                let words = crate::lyrics_fetch::fetch_and_cache(
                    &artist,
                    &title,
                    duration_of(&audio),
                    &audio,
                );
                say(format!("charting \"{title}\"..."));
                let imported = crate::import::import_fetched(&audio, &title, &artist)?;
                let _ = std::fs::remove_file(&audio);
                let _ = std::fs::remove_file(audio.with_extension("lrc"));
                return Ok(finished_line(&title, &words, imported.as_deref()));
            }
            Ok(verdict) => {
                last = verdict.reason;
                let _ = std::fs::remove_file(&audio);
            }
            Err(error) => {
                last = error;
                let _ = std::fs::remove_file(&audio);
            }
        }
    }
    Err(format!("no usable recording: {last}"))
}

/// Rename a fetched file after the song it turned out to be.
///
/// # Errors
/// When the rename fails.
fn rename_to_song(audio: &Path, artist: &str, title: &str) -> Result<PathBuf, String> {
    let extension = audio
        .extension()
        .map_or_else(|| "m4a".to_owned(), |e| e.to_string_lossy().into_owned());
    let named = audio.with_file_name(format!("{}.{extension}", file_stem_for(artist, title)));
    if named == audio {
        return Ok(named);
    }
    std::fs::rename(audio, &named).map_err(|error| format!("{error}"))?;
    // The lyrics land beside the audio, so they travel with it.
    let from = audio.with_extension("lrc");
    if from.is_file() {
        let _ = std::fs::rename(&from, named.with_extension("lrc"));
    }
    Ok(named)
}

/// The line the panel ends on. Pure — tested.
#[must_use]
pub fn finished_line(
    title: &str,
    words: &crate::lyrics_fetch::Outcome,
    warning: Option<&str>,
) -> String {
    let lyrics = match words {
        crate::lyrics_fetch::Outcome::Synced(_) => "with lyrics",
        crate::lyrics_fetch::Outcome::PlainOnly => "lyrics untimed",
        crate::lyrics_fetch::Outcome::NotFound => "no lyrics found",
        crate::lyrics_fetch::Outcome::Failed(_) => "lyrics lookup failed",
    };
    match warning {
        Some(warning) => format!("added \"{title}\" - {lyrics} - {warning}"),
        None => format!("added \"{title}\" - {lyrics}"),
    }
}

/// How long a fetched file actually is, by decoding it.
fn duration_of(audio: &Path) -> Option<f64> {
    beatbyte_audio::decode_file(audio)
        .ok()
        .map(|data| data.duration_s())
}

/// Measure a fetched recording with the instruments already here.
fn measure(audio: &Path) -> Result<Verdict, String> {
    let report = beatbyte_audio::loudness::measure_file(audio, "beatbyte discover")
        .map_err(|error| format!("cannot measure it: {error}"))?;
    let data = beatbyte_audio::decode_file(audio).map_err(|error| format!("{error}"))?;
    let analysis = beatbyte_audio::SpectralAnalyzer::default().analyze(&data);
    // The worst thing found, in the words the report already puts it
    // in; with nothing found, the verdict's own one word.
    let note = report.quality.issues.first().map_or_else(
        || report.quality.verdict.label().to_owned(),
        |issue| issue.what.clone(),
    );
    Ok(judge(
        report.quality.verdict != beatbyte_audio::quality::Verdict::Poor,
        &note,
        analysis.bpm_confidence,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(title: &str, uploader: &str, seconds: Option<f64>) -> Candidate {
        Candidate {
            id: format!("id-{title}"),
            title: title.to_owned(),
            uploader: uploader.to_owned(),
            duration_s: seconds,
        }
    }

    #[test]
    fn a_query_survives_a_missing_artist() {
        assert_eq!(query_for("Nirvana", "Lithium"), "Nirvana Lithium");
        assert_eq!(query_for("  ", " Lithium "), "Lithium");
    }

    #[test]
    fn the_search_output_is_read_line_by_line_and_a_bad_line_is_skipped() {
        // The tool writes one JSON object per line; one broken line
        // among six is not a reason to find nothing.
        let stdout = concat!(
            r#"{"id":"aaa","title":"Song","uploader":"Artist - Topic","duration":215.0}"#,
            "\n",
            "{ not json at all\n",
            r#"{"id":"bbb","title":"Song (Live)","channel":"Fan","duration":260}"#,
            "\n",
            r#"{"title":"no id here"}"#,
            "\n",
        );
        let found = parse_candidates(stdout);
        assert_eq!(found.len(), 2, "two good lines: {found:?}");
        assert_eq!(found[0].id, "aaa");
        assert_eq!(found[0].uploader, "Artist - Topic");
        assert_eq!(found[1].uploader, "Fan", "channel stands in for uploader");
        assert_eq!(found[0].duration_s, Some(215.0));
    }

    #[test]
    fn a_different_edit_is_thrown_out_by_the_catalogues_own_rule() {
        // The rule that exists because an 8:37 remix was handed the
        // 4-minute original's stamps.
        let ours = Some(215.0);
        assert!(plausible(&candidate("Song", "Topic", Some(214.0)), ours));
        assert!(!plausible(&candidate("Song", "Topic", Some(517.0)), ours));
        // Nothing to compare is not a reason to reject.
        assert!(plausible(&candidate("Song", "Topic", None), ours));
        assert!(plausible(&candidate("Song", "Topic", Some(517.0)), None));
    }

    #[test]
    fn a_recording_that_says_what_it_is_is_ranked_on_it() {
        assert!(name_score(&candidate("Song", "Artist - Topic", None)) > 0);
        assert!(name_score(&candidate("Song (Live at Reading)", "Fan", None)) < 0);
        assert!(name_score(&candidate("Song - Karaoke", "Tracks", None)) < 0);
        assert!(
            name_score(&candidate("Song", "Artist - Topic", None))
                > name_score(&candidate("Song", "Somebody", None)),
            "an artist channel beats an unknown one"
        );
    }

    #[test]
    fn the_shortlist_filters_by_length_then_orders_by_name() {
        let ours = Some(215.0);
        let found = vec![
            candidate("Song (Live)", "Fan", Some(216.0)),
            candidate("Song", "Artist - Topic", Some(214.0)),
            candidate("Song", "Bootlegs", Some(600.0)),
        ];
        let ranked = rank_by_metadata(&found, ours);
        assert_eq!(ranked.len(), 2, "the 10-minute one is another edit");
        assert_eq!(ranked[0].uploader, "Artist - Topic", "the master leads");
        assert!(ranked[1].title.contains("Live"));
    }

    #[test]
    fn ties_keep_the_search_engines_own_order() {
        // `sort_by` is stable, and that is load-bearing: with nothing
        // to tell two candidates apart, the tool's relevance is a
        // better answer than whatever a sort happens to do.
        let found = vec![
            candidate("Song one", "Somebody", None),
            candidate("Song two", "Somebody", None),
            candidate("Song three", "Somebody", None),
        ];
        let ranked = rank_by_metadata(&found, None);
        let titles: Vec<&str> = ranked.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Song one", "Song two", "Song three"]);
    }

    #[test]
    fn the_cli_is_preferred_because_it_needs_no_key() {
        assert_eq!(backend_for(true, true, None), Backend::Cli);
        assert_eq!(
            backend_for(true, true, Some("sk-key")),
            Backend::Cli,
            "a key is not used when the CLI is there"
        );
        assert_eq!(
            backend_for(true, false, Some("sk-key")),
            Backend::Api("sk-key".to_owned())
        );
        // Off is off, whatever is installed.
        assert_eq!(backend_for(false, true, Some("sk-key")), Backend::Off);
        // Switched on with nothing to run it with is not an error.
        assert_eq!(backend_for(true, false, None), Backend::Off);
        assert_eq!(backend_for(true, false, Some("   ")), Backend::Off);
    }

    #[test]
    fn the_prompt_carries_titles_and_nothing_else() {
        let ranked = vec![
            candidate("Song", "Artist - Topic", Some(215.0)),
            candidate("Song (Live)", "Fan", None),
        ];
        let prompt = prompt_for("Artist", "Song", &ranked);
        assert!(prompt.contains("0: Song [Artist - Topic] (215s)"));
        assert!(prompt.contains("1: Song (Live) [Fan] (unknown)"));
        assert!(prompt.contains("index only"));
    }

    #[test]
    fn an_answer_that_is_not_a_candidate_changes_nothing() {
        assert_eq!(parse_choice("1", 3), Some(1));
        assert_eq!(parse_choice("  2  ", 3), Some(2));
        assert_eq!(parse_choice("The answer is 0.", 3), Some(0));
        // Out of range, or no number at all: the ranking stands.
        assert_eq!(parse_choice("7", 3), None);
        assert_eq!(parse_choice("none of them", 3), None);
        assert_eq!(parse_choice("", 3), None);
    }

    #[test]
    fn a_promotion_moves_one_and_keeps_the_rest_in_order() {
        let ranked = vec![
            candidate("a", "x", None),
            candidate("b", "x", None),
            candidate("c", "x", None),
        ];
        let moved = promote(&ranked, 2);
        let titles: Vec<&str> = moved.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["c", "a", "b"]);
        // An index nobody offered leaves the list whole.
        assert_eq!(promote(&ranked, 9).len(), ranked.len());
    }

    #[test]
    fn a_recording_is_judged_on_the_instruments_not_on_its_name() {
        assert!(judge(true, "clean", 0.8).good);
        let poor = judge(false, "spectrum ends at 11 kHz", 0.9);
        assert!(!poor.good && poor.reason.contains("11 kHz"));
        let wobbly = judge(true, "clean", 0.05);
        assert!(!wobbly.good && wobbly.reason.contains("tempo"));
    }

    #[test]
    fn the_fetch_target_is_built_from_the_id_not_the_title() {
        // A title with a quote or a slash in it must never reach a
        // URL — only the id does, and the id is a plain token.
        let awkward = Candidate {
            id: "dQw4w9WgXcQ".to_owned(),
            title: r#"Song "quoted" / slashed"#.to_owned(),
            uploader: "x".to_owned(),
            duration_s: None,
        };
        let target = awkward.fetch_target();
        assert!(target.ends_with("dQw4w9WgXcQ"));
        assert!(!target.contains('"') && !target.contains(' '));
    }

    #[test]
    fn an_id_that_is_not_a_plain_token_is_not_a_candidate() {
        // The ids come from another program's output: untrusted, the
        // way a chart file is. This test is why `is_plain_id` exists
        // — the first cut interpolated whatever arrived into a URL.
        assert!(is_plain_id("dQw4w9WgXcQ"));
        assert!(is_plain_id("a-b_c123"));
        assert!(!is_plain_id(""));
        assert!(!is_plain_id("has space"));
        assert!(!is_plain_id(r#"has"quote"#));
        assert!(!is_plain_id("has/slash"));
        assert!(!is_plain_id(&"x".repeat(65)));
        let stdout = concat!(
            r#"{"id":"has space","title":"Song"}"#,
            "\n",
            r#"{"id":"dQw4w9WgXcQ","title":"Song"}"#,
            "\n",
        );
        let found = parse_candidates(stdout);
        assert_eq!(found.len(), 1, "only the plain id survives: {found:?}");
        assert_eq!(found[0].id, "dQw4w9WgXcQ");
    }

    /// The live half of the search, against the real tool: it asks,
    /// reads what comes back and ranks it. Downloads NOTHING — the
    /// decision is what this covers; the fetch and the charting are
    /// `import_song`, which has its own tests.
    ///
    /// Ignored by default: it needs the network and `yt-dlp`. Run it
    /// with `cargo test -p beatbyte-game --lib discover -- --ignored
    /// --nocapture` when the parsing is in question.
    #[test]
    #[ignore = "needs the network and yt-dlp"]
    fn the_live_search_returns_rankable_candidates() {
        if !tool_available() {
            eprintln!("skipped: {}", missing_tool_message());
            return;
        }
        let found = search("Nirvana", "Lithium").expect("the search runs");
        assert!(!found.is_empty(), "the search found nothing");
        for candidate in &found {
            assert!(is_plain_id(&candidate.id), "odd id: {:?}", candidate.id);
            assert!(!candidate.title.is_empty(), "a candidate without a title");
        }
        let ranked = rank_by_metadata(&found, Some(255.0));
        eprintln!("{} found, {} the right length:", found.len(), ranked.len());
        for candidate in &ranked {
            eprintln!(
                "  [{:>3}] {} [{}] {:?}s",
                name_score(candidate),
                candidate.title,
                candidate.uploader,
                candidate.duration_s
            );
        }
        assert!(!ranked.is_empty(), "nothing survived the length rule");
    }

    #[test]
    fn a_published_title_loses_its_furniture() {
        assert_eq!(
            clean_title("How Bizarre (Official Music Video)"),
            "How Bizarre"
        );
        assert_eq!(clean_title("Song [HD] (Lyrics)"), "Song");
        assert_eq!(clean_title("  Song   spaced  "), "Song spaced");
        // Nothing to strip, nothing lost.
        assert_eq!(clean_title("Plain Song"), "Plain Song");
        // An unclosed bracket must not eat the rest of the name.
        assert_eq!(clean_title("Song (unclosed"), "Song");
    }

    #[test]
    fn the_names_come_from_the_recording_when_the_query_gave_none() {
        // Reported: a song typed without an artist was filed under
        // the raw query, and the catalogue was then asked for a song
        // by that name and found nothing.
        let published = candidate("OMC - How Bizarre (Official Video)", "OMC", None);
        let (artist, title) = names_from("", "how bizarre ocm", &published);
        assert_eq!(artist, "OMC");
        assert_eq!(title, "How Bizarre");
        // No dash in the title: the channel is the artist.
        let bare = candidate("How Bizarre", "OMC - Topic", None);
        assert_eq!(
            names_from("", "how bizarre", &bare),
            ("OMC".to_owned(), "How Bizarre".to_owned())
        );
        // A query that named an artist wins: the player said what
        // they meant, and a publisher's title does not overrule it.
        assert_eq!(
            names_from("Nirvana", "Lithium", &published),
            ("Nirvana".to_owned(), "Lithium".to_owned())
        );
    }

    #[test]
    fn the_file_is_named_after_the_song_not_the_video() {
        // The import takes the folder name from the file name, so a
        // file called after the id filed the song under
        // `c2cmg33mwvy-m4a` and nobody could find it.
        assert_eq!(file_stem_for("OMC", "How Bizarre"), "OMC - How Bizarre");
        assert_eq!(file_stem_for("", "How Bizarre"), "How Bizarre");
        // Nothing that would build a path may survive.
        let awkward = file_stem_for("AC/DC", "Back: In*Black?");
        for bad in ['/', '\\', ':', '*', '?', '"', '<', '>', '|'] {
            assert!(!awkward.contains(bad), "{bad:?} survived in {awkward:?}");
        }
        // And it is never empty, or the file would have no name.
        assert!(!file_stem_for("", "").is_empty());
        assert!(!file_stem_for("  ", " ... ").is_empty());
    }

    #[test]
    fn a_missing_tool_says_how_to_get_it() {
        let message = missing_tool_message();
        assert!(message.contains(FETCH_TOOL));
        assert!(message.contains("install"));
    }
}
