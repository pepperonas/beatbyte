//! The play history's schema: one line per played track.
//!
//! Lives here, not in the game crate, for the reason the telemetry
//! schema does: the game WRITES these files and the CLI READS them,
//! so the format has exactly one definition and neither side can
//! drift from the other.

use serde::{Deserialize, Serialize};

use crate::player::PlayerId;

/// What one player's run looked like beyond the score.
///
/// Every field is optional and defaults to absent, because every
/// field was added after the log existed: a run recorded before they
/// did knows `None`, which reads as "not recorded" — never as a
/// zero, which would be a lie a statistic would then average in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RunDetail {
    /// Longest streak in the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_streak: Option<u32>,
    /// Notes judged perfect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perfect: Option<u32>,
    /// Notes judged great.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub great: Option<u32>,
    /// Notes judged good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub good: Option<u32>,
    /// Notes missed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss: Option<u32>,
    /// Strums that matched no note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overstrums: Option<u32>,
    /// Mean signed `hit - note` offset in milliseconds: negative is
    /// early, positive late. The one number that says whether a
    /// player drifts, and whether a calibration helped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_offset_ms: Option<f64>,
}

impl RunDetail {
    /// Notes judged at all — the denominator the shares are taken
    /// over. `None` when the run predates the counts. Pure — tested.
    #[must_use]
    pub fn judged(&self) -> Option<u32> {
        match (self.perfect, self.great, self.good, self.miss) {
            (Some(p), Some(gr), Some(go), Some(m)) => Some(p + gr + go + m),
            _ => None,
        }
    }
}

/// One player's part in one run: slot one's numbers, or a
/// co-player's. The two are the same shape on purpose — a statistic
/// that treated slot one as the real player and the rest as an
/// afterthought would be wrong about every co-op night.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunPart {
    /// Who played it, when the roster knew someone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<PlayerId>,
    /// Score achieved.
    pub score: u64,
    /// Weighted accuracy, 0.0–1.0.
    pub accuracy: f64,
    /// The rest of the run's numbers.
    #[serde(default)]
    pub detail: RunDetail,
}

/// One played track.
///
/// Title and artist are separate fields, never joined: the score
/// board's `title|artist` key is a known collision (roadmap C5) and
/// the telemetry schema already refuses to copy it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayEntry {
    /// Song title, as the chart carries it.
    pub title: String,
    /// Song artist, as the chart carries it.
    pub artist: String,
    /// Difficulty played, lowercase display name.
    pub difficulty: String,
    /// Unix milliseconds when the run started.
    pub started_ms: u64,
    /// How long the track actually ran, in wall-clock seconds.
    ///
    /// Wall clock, not song time: at 50 % practice speed a track is
    /// audible for twice its length, and "how long was this
    /// performed" is the question a reporting body asks.
    pub played_s: f64,
    /// The song's own length, when the chart knows it. Together with
    /// `played_s` this says whether the track ran through or was
    /// left early — without the reader having to trust a flag alone.
    pub track_s: Option<f64>,
    /// Whether the run reached the end of the song.
    pub completed: bool,
    /// How many players were on the highway.
    pub players: usize,
    /// Practice speed or a section loop was used at some point.
    pub practice: bool,
    /// The autopilot was driving (test runs, not performances).
    pub autopilot: bool,
    /// Score of player one — the analysis side of the log.
    pub score: u64,
    /// Weighted accuracy of player one, 0.0–1.0.
    pub accuracy: f64,
    /// Where the audio came from: `builtin` or `file`.
    pub source: String,
    /// Who played slot one, when the roster knew someone. Absent for
    /// every run played before the roster existed, and for a run
    /// played with nobody selected — "unattributed" is a fact worth
    /// keeping, not a gap to fill with a guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<PlayerId>,
    /// Slot one's numbers beyond score and accuracy.
    #[serde(default)]
    pub detail: RunDetail,
    /// Slots two and up. Empty for a solo run, so a solo log line is
    /// exactly as long as it always was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub co_players: Vec<RunPart>,
    /// Content hash of the exact chart that was played.
    ///
    /// Without it a rising accuracy cannot be told apart from a
    /// chart that was redesigned easier underneath the player — and
    /// this library is redesigned in rollovers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_hash: Option<String>,
}

impl PlayEntry {
    /// Every player's part in this run, slot one first.
    ///
    /// The uniform view statistics read: slot one's fields are flat
    /// on the entry for the log's own history, and this is what
    /// hides that seam from every reader. Pure — tested.
    #[must_use]
    pub fn parts(&self) -> Vec<RunPart> {
        let mut parts = Vec::with_capacity(1 + self.co_players.len());
        parts.push(RunPart {
            player: self.player,
            score: self.score,
            accuracy: self.accuracy,
            detail: self.detail,
        });
        parts.extend(self.co_players.iter().cloned());
        parts
    }

    /// This player's part in this run, if they were in it.
    #[must_use]
    pub fn part_of(&self, player: PlayerId) -> Option<RunPart> {
        self.parts().into_iter().find(|p| p.player == Some(player))
    }
}

/// Serialize one entry as a log line (no trailing newline). Pure.
///
/// # Errors
/// When the entry cannot be serialized, which for this plain struct
/// means a `serde_json` bug rather than bad input.
pub fn render_entry(entry: &PlayEntry) -> Result<String, serde_json::Error> {
    serde_json::to_string(entry)
}

/// Rewrite a log, crediting unattributed runs to a player.
///
/// Returns the new text and how many lines changed. Used once, when
/// the first player is created: the play log predates the roster, and
/// those runs belong to somebody — without this the first player's
/// statistics open empty beside a log full of their own play.
///
/// Three rules, each of which is a way this could destroy data:
/// autopilot runs are never claimed (they are not a person's play), a
/// run already credited to someone is left alone, and **a line this
/// reader cannot parse is copied through byte for byte** rather than
/// dropped — the log is appended to across crashes, and the reader is
/// required to survive a half-written record. Idempotent: running it
/// twice changes nothing the second time. Pure — tested.
#[must_use]
pub fn claim_unattributed(text: &str, player: crate::player::PlayerId) -> (String, usize) {
    let mut out = String::with_capacity(text.len() + 32);
    let mut claimed = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<PlayEntry>(line) {
            Ok(mut entry) if entry.player.is_none() && !entry.autopilot => {
                entry.player = Some(player);
                match render_entry(&entry) {
                    Ok(rendered) => {
                        out.push_str(&rendered);
                        claimed += 1;
                    }
                    Err(_) => out.push_str(line),
                }
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    (out, claimed)
}

/// Read a whole log, skipping lines that do not parse.
///
/// A history is appended to over years and read by tools that were
/// written later: one damaged line (a half-written record after a
/// crash, a field from a newer version) must cost that line and
/// nothing else. Pure — tested.
#[must_use]
pub fn parse_log(text: &str) -> Vec<PlayEntry> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// A CSV field, quoted when it has to be.
///
/// Titles come from file names and tags: they contain commas,
/// quotes and the occasional newline, and a report that splits a
/// title across two columns is worse than no report.
#[must_use]
pub fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// The date-time a row carries: UTC, ISO 8601, seconds resolution.
///
/// Computed here rather than pulled from a date crate — the workspace
/// has none, and this is the civil-calendar arithmetic from
/// Howard Hinnant's `civil_from_days`, which is exact for every day
/// this program can be handed. Pure — tested against known stamps.
#[must_use]
pub fn iso_utc(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Shift the epoch to 0000-03-01 so leap days land at the end.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The CSV header, and the columns every row follows.
///
/// Deliberately unchanged by the roster: this export answers "what
/// was performed, when, for how long" for a reporting body, which is
/// one row per PERFORMANCE. Who played it — and a co-op night's
/// second and third player — is a question the statistics answer,
/// and bolting it on here would break every reader of the format.
pub const CSV_HEADER: &str = "started_utc,title,artist,seconds_played,track_seconds,completed,difficulty,players,practice,autopilot,source,score,accuracy";

/// Render the history as CSV — the reporting format. Pure — tested.
#[must_use]
pub fn to_csv(entries: &[PlayEntry]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for entry in entries {
        out.push_str(&format!(
            "{},{},{},{:.1},{},{},{},{},{},{},{},{},{:.4}\n",
            iso_utc(entry.started_ms),
            csv_field(&entry.title),
            csv_field(&entry.artist),
            entry.played_s,
            entry
                .track_s
                .map_or_else(String::new, |seconds| format!("{seconds:.1}")),
            entry.completed,
            csv_field(&entry.difficulty),
            entry.players,
            entry.practice,
            entry.autopilot,
            csv_field(&entry.source),
            entry.score,
            entry.accuracy,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(title: &str, started_ms: u64, played_s: f64) -> PlayEntry {
        PlayEntry {
            title: title.to_owned(),
            started_ms,
            played_s,
            ..entry()
        }
    }

    fn entry() -> PlayEntry {
        PlayEntry {
            title: "Synthetic Song".to_owned(),
            artist: "The Null Pointers".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms: 1_756_000_000_000,
            played_s: 64.5,
            track_s: Some(65.0),
            completed: true,
            players: 1,
            practice: false,
            autopilot: false,
            score: 12_345,
            accuracy: 0.93,
            source: "file".to_owned(),
            player: None,
            detail: RunDetail::default(),
            co_players: Vec::new(),
            chart_hash: None,
        }
    }

    /// One log line, as the real log writes it.
    fn claim_line(player: Option<crate::player::PlayerId>, autopilot: bool) -> String {
        let mut entry = entry();
        entry.player = player;
        entry.autopilot = autopilot;
        render_entry(&entry).expect("a plain struct serializes")
    }

    #[test]
    fn the_adoption_claims_human_runs_and_leaves_the_rest() {
        let log = format!(
            "{}\n{}\n{}\n",
            claim_line(None, false),    // a human run with nobody on it
            claim_line(None, true),     // autopilot: never a person's play
            claim_line(Some(9), false)  // already someone else's
        );
        let (out, claimed) = claim_unattributed(&log, 1);
        assert_eq!(claimed, 1);
        let entries = parse_log(&out);
        assert_eq!(entries[0].player, Some(1));
        assert_eq!(entries[1].player, None, "the autopilot run was claimed");
        assert_eq!(entries[2].player, Some(9), "someone else's run was taken");
    }

    #[test]
    fn a_damaged_line_survives_the_adoption_untouched() {
        // The log is appended to across crashes and this reader is
        // required to survive one half-written record. An adoption
        // that dropped it would be a silent data loss.
        let damaged = r#"{"title":"Half a reco"#;
        let log = format!(
            "{}\n{damaged}\n{}\n",
            claim_line(None, false),
            claim_line(None, false)
        );
        let (out, claimed) = claim_unattributed(&log, 7);
        assert_eq!(claimed, 2);
        assert!(out.contains(damaged), "the damaged line was dropped");
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn claiming_twice_changes_nothing_the_second_time() {
        // The roster's flag is what stops a second run, but the
        // rewrite has to be idempotent too: a crash between the
        // rewrite and the save must not double-credit anyone.
        let log = format!("{}\n", claim_line(None, false));
        let (once, first) = claim_unattributed(&log, 3);
        let (twice, second) = claim_unattributed(&once, 3);
        assert_eq!((first, second), (1, 0));
        assert_eq!(once, twice);
    }

    #[test]
    fn an_empty_log_is_nothing_to_adopt() {
        assert_eq!(claim_unattributed("", 1), (String::new(), 0));
        assert_eq!(claim_unattributed("\n\n", 1), (String::new(), 0));
    }

    #[test]
    fn a_run_splits_into_its_players_slot_one_first() {
        let mut solo = entry();
        assert_eq!(solo.parts().len(), 1, "a solo run is one part");
        solo.player = Some(4);
        solo.co_players.push(RunPart {
            player: Some(5),
            score: 10,
            accuracy: 0.5,
            detail: RunDetail::default(),
        });
        let parts = solo.parts();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].player, Some(4), "slot one comes first");
        assert_eq!(solo.part_of(5).map(|p| p.score), Some(10));
        assert_eq!(solo.part_of(99), None);
    }

    #[test]
    fn a_detail_counts_what_it_has_and_admits_what_it_lacks() {
        let full = RunDetail {
            perfect: Some(10),
            great: Some(3),
            good: Some(2),
            miss: Some(1),
            ..RunDetail::default()
        };
        assert_eq!(full.judged(), Some(16));
        // A run recorded before the counts existed cannot report a
        // denominator, and must not invent one.
        assert_eq!(RunDetail::default().judged(), None);
    }

    #[test]
    fn a_title_with_a_comma_stays_one_column() {
        // Titles come from file names and tags. A report that splits
        // a title across two columns is worse than no report.
        assert_eq!(csv_field("Plain"), "Plain");
        assert_eq!(csv_field("Hello, World"), "\"Hello, World\"");
        assert_eq!(csv_field("She said \"hi\""), "\"She said \"\"hi\"\"\"");
        let rows = to_csv(&[titled("Comma, Song", 0, 10.0)]);
        let line = rows.lines().nth(1).expect("one row");
        // Counted the way a reader counts them - commas inside
        // quotes are text, not separators. (Counting raw commas is
        // what the first version of this test did, and it failed on
        // correct output.)
        let fields = split_csv(line);
        assert_eq!(fields.len(), CSV_HEADER.split(',').count());
        assert_eq!(fields[1], "Comma, Song", "the title stayed one field");
    }

    #[test]
    fn the_csv_carries_what_a_report_asks_for() {
        let csv = to_csv(&[titled("Synthetic", 1_756_684_800_000, 64.5)]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(CSV_HEADER));
        let row = lines.next().expect("one row");
        // The work, when it was performed, and for how long.
        assert!(row.starts_with("2025-09-01T00:00:00Z,Synthetic,The Null Pointers,64.5"));
    }

    #[test]
    fn the_timestamp_matches_known_dates() {
        // Fixed points, including a leap day and the epoch itself -
        // the calendar arithmetic is hand-written, so it gets
        // checked against dates that can be looked up.
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_000), "1970-01-01T00:00:01Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_756_684_800_000), "2025-09-01T00:00:00Z");
        assert_eq!(iso_utc(1_735_689_599_000), "2024-12-31T23:59:59Z");
    }

    /// Split one CSV row the way a spreadsheet does: quoted fields
    /// keep their commas, doubled quotes are one quote.
    fn split_csv(line: &str) -> Vec<String> {
        let (mut fields, mut current, mut quoted) = (Vec::new(), String::new(), false);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    current.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => fields.push(std::mem::take(&mut current)),
                other => current.push(other),
            }
        }
        fields.push(current);
        fields
    }

    #[test]
    fn an_entry_round_trips_through_a_log_line() {
        let line = render_entry(&entry()).expect("serializes");
        assert_eq!(parse_log(&line), vec![entry()]);
        // One line per run: a record must never span two lines, or
        // the append-only format falls apart.
        assert!(!line.contains('\n'));
    }

    #[test]
    fn a_damaged_line_costs_only_itself() {
        // Histories are appended to for years and read by tools
        // written later; a half-written record after a crash must
        // not take the rest of the log with it.
        let good = render_entry(&entry()).expect("serializes");
        let log = format!("{good}\nnot json at all\n\n{{\"title\":\"only a title\"}}\n{good}\n");
        assert_eq!(
            parse_log(&log).len(),
            2,
            "the two intact records survive, the broken ones are skipped"
        );
    }

    #[test]
    fn the_flags_a_report_filters_on_are_all_recorded() {
        // The recorder deliberately keeps runs that a report may
        // want to exclude - practice, autopilot, aborted - because
        // dropping them here would be unrecoverable, while filtering
        // them at export is one flag. This pins that each of those
        // facts actually survives into the log.
        let aborted = PlayEntry {
            completed: false,
            practice: true,
            autopilot: true,
            played_s: 3.0,
            ..entry()
        };
        let line = render_entry(&aborted).expect("serializes");
        let back = &parse_log(&line)[0];
        assert!(!back.completed && back.practice && back.autopilot);
        assert!((back.played_s - 3.0).abs() < f64::EPSILON);
    }
}
