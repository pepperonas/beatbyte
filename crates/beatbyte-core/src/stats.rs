//! What a play log says about a player.
//!
//! Every function here is pure: play entries in, numbers out. That is
//! deliberate — the game draws these numbers and the CLI prints them,
//! and two implementations of "accuracy over time" would eventually
//! disagree in front of the player.
//!
//! **Two rules run through the whole module.**
//!
//! *Autopilot runs are never counted.* The harness plays perfectly;
//! a statistic that averaged it in would tell every player they are
//! flawless. [`Filter`] cannot be configured to include them.
//!
//! *A missing number is not a zero.* Runs recorded before the log
//! carried streaks, judgments and drift read as `None`, and every
//! aggregate below skips them rather than averaging in a zero — so a
//! year-old history lowers no average it was never part of.

use std::collections::{BTreeMap, BTreeSet};

use crate::difficulty::Difficulty;
use crate::history::{PlayEntry, RunPart};
use crate::player::PlayerId;

/// Which runs an aggregate is taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    /// Count runs where practice speed or a section loop was used.
    ///
    /// On by default: a track practised at half speed was still
    /// played, and hiding it makes an activity chart lie. The runs
    /// stay flagged in the log, so the screen can offer the switch.
    pub practice: bool,
    /// Count only runs that reached the end of the song.
    ///
    /// The quality metrics (accuracy, drift, streak) use this: an
    /// abandoned run's accuracy describes the eight bars that were
    /// played, not the song, and mixing the two makes a trend line
    /// meaningless.
    pub completed_only: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            practice: true,
            completed_only: false,
        }
    }
}

impl Filter {
    /// The filter the quality metrics use: finished runs only.
    #[must_use]
    pub const fn finished() -> Filter {
        Filter {
            practice: true,
            completed_only: true,
        }
    }

    /// Whether this run passes. Autopilot never does. Pure — tested.
    #[must_use]
    pub const fn keeps(&self, entry: &PlayEntry) -> bool {
        !entry.autopilot
            && (self.practice || !entry.practice)
            && (!self.completed_only || entry.completed)
    }
}

/// One player's run: the entry it happened in, and their part in it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRun<'a> {
    /// The logged run.
    pub entry: &'a PlayEntry,
    /// What this player did in it.
    pub part: RunPart,
}

impl PlayerRun<'_> {
    /// The difficulty played, when the log's text names one.
    #[must_use]
    pub fn difficulty(&self) -> Option<Difficulty> {
        Difficulty::from_id(&self.entry.difficulty)
    }

    /// The song, as a key that cannot collide (title and artist stay
    /// separate fields — the scoreboard's old `title|artist` string
    /// is the collision this avoids).
    #[must_use]
    pub fn song(&self) -> (&str, &str) {
        (&self.entry.title, &self.entry.artist)
    }
}

/// Every run a player took part in, oldest first.
///
/// Slot one and co-players alike: being player two on a co-op night
/// is still having played. Pure — tested.
#[must_use]
pub fn runs_of<'a>(
    entries: &'a [PlayEntry],
    player: PlayerId,
    filter: Filter,
) -> Vec<PlayerRun<'a>> {
    let mut runs: Vec<PlayerRun<'a>> = entries
        .iter()
        .filter(|entry| filter.keeps(entry))
        .filter_map(|entry| entry.part_of(player).map(|part| PlayerRun { entry, part }))
        .collect();
    runs.sort_by_key(|run| run.entry.started_ms);
    runs
}

/// The runs nobody is credited with: everything played before the
/// roster existed, or with nobody selected. Pure — tested.
#[must_use]
pub fn unattributed(entries: &[PlayEntry], filter: Filter) -> Vec<&PlayEntry> {
    entries
        .iter()
        .filter(|entry| filter.keeps(entry) && entry.parts().iter().all(|p| p.player.is_none()))
        .collect()
}

/// The headline numbers for one player.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    /// Runs taken part in.
    pub runs: usize,
    /// Runs that reached the end.
    pub completed: usize,
    /// Distinct songs played.
    pub songs: usize,
    /// Wall-clock seconds spent playing.
    pub seconds: f64,
    /// Highest score in a FINISHED run. `None` when none finished.
    pub best_score: Option<u64>,
    /// Highest accuracy in a FINISHED run, 0.0–1.0.
    pub best_accuracy: Option<f64>,
    /// Mean accuracy over the FINISHED runs, 0.0–1.0.
    pub mean_accuracy: Option<f64>,
    /// Longest streak the log recorded.
    pub best_streak: Option<u32>,
    /// Mean of the runs' mean offsets, in milliseconds.
    pub mean_offset_ms: Option<f64>,
    /// Overstrums per minute played.
    pub overstrums_per_min: Option<f64>,
    /// Share of judged notes that were perfect, 0.0–1.0.
    pub perfect_share: Option<f64>,
    /// When the player last played.
    pub last_played_ms: Option<u64>,
}

/// Fold a player's runs into [`Summary`].
///
/// Volume (runs, songs, seconds) counts every run that passes the
/// filter. **Quality (accuracy, score) counts only FINISHED runs**,
/// and that is not a detail: an abandoned run is logged with
/// `accuracy: 0.0` because nothing was scored, which is
/// indistinguishable from playing terribly. Averaging those in put
/// this player's mean at 14.6 % when their finished runs average
/// 45 % — the module's own "a missing number is not a zero" rule,
/// broken at the first opportunity and caught by printing the
/// numbers next to the log. Pure — tested.
#[must_use]
pub fn summarize(runs: &[PlayerRun<'_>]) -> Summary {
    let mut summary = Summary::default();
    let mut songs: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut accuracies = Sum::default();
    let mut offsets = Sum::default();
    let mut overstrums: Option<u64> = None;
    let (mut perfect, mut judged) = (0_u64, 0_u64);

    for run in runs {
        summary.runs += 1;
        summary.completed += usize::from(run.entry.completed);
        summary.seconds += run.entry.played_s;
        songs.insert(run.song());
        if run.entry.completed {
            summary.best_score = Some(
                summary
                    .best_score
                    .map_or(run.part.score, |b| b.max(run.part.score)),
            );
            summary.best_accuracy = Some(
                summary
                    .best_accuracy
                    .map_or(run.part.accuracy, |b| b.max(run.part.accuracy)),
            );
            accuracies.push(run.part.accuracy);
        }
        summary.last_played_ms = Some(
            summary
                .last_played_ms
                .map_or(run.entry.started_ms, |ms| ms.max(run.entry.started_ms)),
        );
        let detail = &run.part.detail;
        if let Some(streak) = detail.best_streak {
            summary.best_streak = Some(summary.best_streak.map_or(streak, |b| b.max(streak)));
        }
        if let Some(offset) = detail.mean_offset_ms {
            offsets.push(offset);
        }
        if let Some(count) = detail.overstrums {
            overstrums = Some(overstrums.unwrap_or(0) + u64::from(count));
        }
        if let (Some(p), Some(total)) = (detail.perfect, detail.judged()) {
            perfect += u64::from(p);
            judged += u64::from(total);
        }
    }

    summary.songs = songs.len();
    summary.mean_accuracy = accuracies.mean();
    summary.mean_offset_ms = offsets.mean();
    // Per minute, not per run: a two-minute song and a six-minute one
    // are not the same opportunity to overstrum.
    summary.overstrums_per_min = overstrums
        .and_then(|count| (summary.seconds > 0.0).then(|| count as f64 / (summary.seconds / 60.0)));
    summary.perfect_share = (judged > 0).then(|| perfect as f64 / judged as f64);
    summary
}

/// A running mean that knows whether it saw anything.
#[derive(Debug, Default, Clone, Copy)]
struct Sum {
    total: f64,
    count: usize,
}

impl Sum {
    fn push(&mut self, value: f64) {
        if value.is_finite() {
            self.total += value;
            self.count += 1;
        }
    }

    fn mean(self) -> Option<f64> {
        (self.count > 0).then(|| self.total / self.count as f64)
    }
}

/// One difficulty's slice of a player's play.
#[derive(Debug, Clone, PartialEq)]
pub struct DifficultyStat {
    /// Which difficulty.
    pub difficulty: Difficulty,
    /// Runs at it.
    pub runs: usize,
    /// Runs at it that reached the end.
    pub completed: usize,
    /// Mean accuracy over the FINISHED runs at it.
    pub mean_accuracy: Option<f64>,
    /// Best accuracy in a FINISHED run at it.
    pub best_accuracy: Option<f64>,
    /// Best score in a FINISHED run at it.
    pub best_score: Option<u64>,
}

impl DifficultyStat {
    /// Share of runs at this difficulty that were finished, 0.0–1.0.
    /// `None` when none were played — an empty bar is not a zero bar.
    #[must_use]
    pub fn completion(&self) -> Option<f64> {
        (self.runs > 0).then(|| self.completed as f64 / self.runs as f64)
    }
}

/// A player's play split by difficulty, easiest first. Every
/// difficulty appears, played or not — a chart with a gap where
/// Expert should be says more than a chart with three bars. Pure —
/// tested.
#[must_use]
pub fn by_difficulty(runs: &[PlayerRun<'_>]) -> Vec<DifficultyStat> {
    Difficulty::ALL
        .into_iter()
        .map(|difficulty| {
            let mine: Vec<&PlayerRun<'_>> = runs
                .iter()
                .filter(|run| run.difficulty() == Some(difficulty))
                .collect();
            let mut accuracies = Sum::default();
            let mut stat = DifficultyStat {
                difficulty,
                runs: mine.len(),
                completed: mine.iter().filter(|run| run.entry.completed).count(),
                mean_accuracy: None,
                best_accuracy: None,
                best_score: None,
            };
            // Finished runs only, for the reason `summarize`
            // documents: an abandoned run scored nothing and says
            // nothing about how well this difficulty is played.
            for run in mine.into_iter().filter(|run| run.entry.completed) {
                accuracies.push(run.part.accuracy);
                stat.best_accuracy = Some(
                    stat.best_accuracy
                        .map_or(run.part.accuracy, |b: f64| b.max(run.part.accuracy)),
                );
                stat.best_score = Some(
                    stat.best_score
                        .map_or(run.part.score, |b: u64| b.max(run.part.score)),
                );
            }
            stat.mean_accuracy = accuracies.mean();
            stat
        })
        .collect()
}

/// One finished run, as a point on a time axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// When the run started, unix milliseconds.
    pub started_ms: u64,
    /// The difficulty it was played at, when known.
    pub difficulty: Option<Difficulty>,
    /// Weighted accuracy, 0.0–1.0.
    pub accuracy: f64,
    /// Score.
    pub score: u64,
    /// Mean signed offset, when recorded.
    pub mean_offset_ms: Option<f64>,
}

/// A player's finished runs as points, oldest first.
///
/// Finished runs only, and that is the whole point: an abandoned
/// run's accuracy describes the part that was played and would show
/// up on a progress line as noise. Pure — tested.
#[must_use]
pub fn progression(runs: &[PlayerRun<'_>]) -> Vec<Point> {
    runs.iter()
        .filter(|run| run.entry.completed)
        .map(|run| Point {
            started_ms: run.entry.started_ms,
            difficulty: run.difficulty(),
            accuracy: run.part.accuracy,
            score: run.part.score,
            mean_offset_ms: run.part.detail.mean_offset_ms,
        })
        .collect()
}

/// The least-squares slope through `(index, value)` pairs — "how much
/// per run", the number a trend line states.
///
/// `None` below three points: two points always fit a line exactly,
/// and calling that a trend would be a claim the data cannot make.
/// Pure — tested.
#[must_use]
pub fn trend(values: &[f64]) -> Option<f64> {
    let usable: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if usable.len() < 3 {
        return None;
    }
    let n = usable.len() as f64;
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = usable.iter().sum::<f64>() / n;
    let mut covariance = 0.0;
    let mut variance = 0.0;
    for (index, value) in usable.iter().enumerate() {
        let dx = index as f64 - mean_x;
        covariance += dx * (value - mean_y);
        variance += dx * dx;
    }
    (variance > 0.0).then(|| covariance / variance)
}

/// A song two players have both finished, and how each did on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Duel {
    /// Song title.
    pub title: String,
    /// Song artist.
    pub artist: String,
    /// The difficulty both played.
    pub difficulty: String,
    /// The first player's best accuracy on it.
    pub theirs: f64,
    /// The second player's best accuracy on it.
    pub others: f64,
    /// The first player's best score on it.
    pub their_score: u64,
    /// The second player's best score on it.
    pub other_score: u64,
}

impl Duel {
    /// Accuracy difference, positive when the first player is ahead.
    #[must_use]
    pub fn margin(&self) -> f64 {
        self.theirs - self.others
    }
}

/// Where two players have played the same song at the same
/// difficulty — the only comparison this module will make.
///
/// A single "who is better" number would need difficulty weights
/// that nothing in this game measures; inventing them would make
/// every ranking an opinion wearing a decimal point. A duel compares
/// like with like or says nothing, which is why the list can come
/// back empty and the screen has to say so. Both players' BEST run
/// on the song counts, not their latest. Pure — tested.
#[must_use]
pub fn head_to_head(entries: &[PlayEntry], theirs: PlayerId, others: PlayerId) -> Vec<Duel> {
    let mine = bests(entries, theirs);
    let yours = bests(entries, others);
    let mut duels: Vec<Duel> = mine
        .iter()
        .filter_map(|(key, best)| {
            yours.get(key).map(|other| Duel {
                title: key.0.clone(),
                artist: key.1.clone(),
                difficulty: key.2.clone(),
                theirs: best.accuracy,
                others: other.accuracy,
                their_score: best.score,
                other_score: other.score,
            })
        })
        .collect();
    // Biggest margin first, in either direction: the songs where the
    // two differ most are what a comparison is for.
    duels.sort_by(|a, b| {
        b.margin()
            .abs()
            .partial_cmp(&a.margin().abs())
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    duels
}

/// A player's best run on one song at one difficulty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Best {
    /// Best accuracy reached.
    pub accuracy: f64,
    /// Best score reached.
    pub score: u64,
}

/// Song + difficulty, the key a personal best is held under.
pub type SongKey = (String, String, String);

/// A player's best finished run per song and difficulty.
///
/// Derived from the log rather than read from the scoreboard on
/// purpose: `scores.json` holds one record per song for the MACHINE,
/// with no idea who set it. Pure — tested.
#[must_use]
pub fn bests(entries: &[PlayEntry], player: PlayerId) -> BTreeMap<SongKey, Best> {
    let mut out: BTreeMap<SongKey, Best> = BTreeMap::new();
    for run in runs_of(entries, player, Filter::finished()) {
        let key = (
            run.entry.title.clone(),
            run.entry.artist.clone(),
            run.entry.difficulty.clone(),
        );
        let best = out.entry(key).or_insert(Best {
            accuracy: 0.0,
            score: 0,
        });
        best.accuracy = best.accuracy.max(run.part.accuracy);
        best.score = best.score.max(run.part.score);
    }
    out
}

/// How many runs fell on each day, oldest day first.
///
/// Days are UTC, counted off the same epoch arithmetic the history's
/// ISO stamps use, so an activity chart and an exported row can never
/// disagree about which day a run belongs to. Pure — tested.
#[must_use]
pub fn per_day(runs: &[PlayerRun<'_>]) -> Vec<(u64, usize)> {
    let mut days: BTreeMap<u64, usize> = BTreeMap::new();
    for run in runs {
        *days.entry(run.entry.started_ms / 86_400_000).or_insert(0) += 1;
    }
    days.into_iter().collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::history::RunDetail;

    fn run(started_ms: u64, player: Option<PlayerId>, accuracy: f64) -> PlayEntry {
        PlayEntry {
            title: "Song".to_owned(),
            artist: "Band".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms,
            played_s: 120.0,
            track_s: Some(120.0),
            completed: true,
            players: 1,
            practice: false,
            autopilot: false,
            score: 1000,
            accuracy,
            source: "file".to_owned(),
            player,
            detail: RunDetail::default(),
            co_players: Vec::new(),
            chart_hash: None,
            genre: None,
            tap_mode: None,
            no_fail: None,
            speed_percent: None,
        }
    }

    #[test]
    fn the_autopilot_never_counts_however_the_filter_is_set() {
        // It plays perfectly. Averaged in, it would tell every player
        // they are flawless — and 78 % of this machine's log is it.
        let mut bot = run(1, Some(1), 1.0);
        bot.autopilot = true;
        let human = run(2, Some(1), 0.5);
        for filter in [
            Filter::default(),
            Filter::finished(),
            Filter {
                practice: false,
                completed_only: false,
            },
        ] {
            let log = [bot.clone(), human.clone()];
            let runs = runs_of(&log, 1, filter);
            assert_eq!(runs.len(), 1, "the autopilot run was counted");
            assert_eq!(summarize(&runs).mean_accuracy, Some(0.5));
        }
    }

    #[test]
    fn a_co_player_has_played_too() {
        // Being player two on a co-op night is having played, and the
        // numbers live in `co_players`, not in the flat fields.
        let mut entry = run(1, Some(1), 0.9);
        entry.players = 2;
        entry.co_players.push(RunPart {
            player: Some(2),
            score: 777,
            accuracy: 0.4,
            detail: RunDetail::default(),
        });
        let second = runs_of(std::slice::from_ref(&entry), 2, Filter::default());
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].part.score, 777);
        assert_eq!(summarize(&second).mean_accuracy, Some(0.4));
        // …and slot one still reads as slot one.
        let first = runs_of(std::slice::from_ref(&entry), 1, Filter::default());
        assert!((first[0].part.accuracy - 0.9).abs() < 1e-9);
    }

    #[test]
    fn a_missing_number_lowers_no_average() {
        // Old runs carry no streak and no drift. Counting them as 0
        // would invent a bad night that never happened.
        let mut old = run(1, Some(1), 0.8);
        let mut new = run(2, Some(1), 0.8);
        new.detail = RunDetail {
            best_streak: Some(100),
            mean_offset_ms: Some(20.0),
            ..RunDetail::default()
        };
        old.detail = RunDetail::default();
        let log = [old, new];
        let summary = summarize(&runs_of(&log, 1, Filter::default()));
        assert_eq!(summary.best_streak, Some(100));
        assert_eq!(summary.mean_offset_ms, Some(20.0), "averaged with a 0");
    }

    #[test]
    fn volume_counts_every_run_and_quality_only_the_finished_ones() {
        let mut abandoned = run(1, Some(1), 0.2);
        abandoned.completed = false;
        abandoned.played_s = 10.0;
        let finished = run(2, Some(1), 0.9);
        let log = [abandoned, finished];
        let all = runs_of(&log, 1, Filter::default());
        let summary = summarize(&all);
        assert_eq!(summary.runs, 2, "both runs happened");
        assert_eq!(summary.completed, 1);
        assert!((summary.seconds - 130.0).abs() < 1e-9);
        // …but the abandoned run's 0.2 must not drag the average.
        // An abandoned run is logged with whatever was scored before
        // it stopped — on this machine, 39 such runs put a player's
        // mean at 14.6 % against a real 45 %.
        assert_eq!(
            summary.mean_accuracy,
            Some(0.9),
            "an abandoned run was averaged in"
        );
        assert_eq!(summary.best_accuracy, Some(0.9));
        // The progress line only takes the finished one.
        assert_eq!(progression(&all).len(), 1);
    }

    #[test]
    fn overstrums_are_per_minute_not_per_run() {
        // A six-minute song is not the same opportunity as a
        // two-minute one; per run, the long song looks sloppier.
        let mut short = run(1, Some(1), 0.9);
        short.played_s = 60.0;
        short.detail.overstrums = Some(6);
        let log = [short];
        let summary = summarize(&runs_of(&log, 1, Filter::default()));
        assert!((summary.overstrums_per_min.unwrap() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn a_trend_needs_more_than_two_points() {
        // Two points fit a line exactly; calling that a trend is a
        // claim the data cannot make.
        assert_eq!(trend(&[0.1, 0.9]), None);
        assert_eq!(trend(&[]), None);
        let rising = trend(&[0.0, 1.0, 2.0, 3.0]).unwrap();
        assert!((rising - 1.0).abs() < 1e-9);
        let falling = trend(&[3.0, 2.0, 1.0, 0.0]).unwrap();
        assert!((falling + 1.0).abs() < 1e-9);
        // A flat line trends at zero, it does not vanish.
        assert_eq!(trend(&[0.5, 0.5, 0.5, 0.5]), Some(0.0));
    }

    #[test]
    fn a_duel_needs_the_same_song_at_the_same_difficulty() {
        let mut mine = run(1, Some(1), 0.9);
        let mut theirs = run(2, Some(2), 0.7);
        // Same song, different difficulty: not comparable.
        theirs.difficulty = "hard".to_owned();
        assert!(head_to_head(&[mine.clone(), theirs.clone()], 1, 2).is_empty());
        // Same song, same difficulty: one duel, and the margin says
        // who is ahead.
        theirs.difficulty = "medium".to_owned();
        let duels = head_to_head(&[mine.clone(), theirs.clone()], 1, 2);
        assert_eq!(duels.len(), 1);
        assert!((duels[0].margin() - 0.2).abs() < 1e-9);
        // The BEST run counts, not the latest.
        mine.accuracy = 0.5;
        let mut better = run(3, Some(1), 0.95);
        better.title = mine.title.clone();
        let duels = head_to_head(&[mine, theirs, better], 1, 2);
        assert!((duels[0].theirs - 0.95).abs() < 1e-9);
    }

    #[test]
    fn every_difficulty_appears_played_or_not() {
        // A gap where Expert should be says more than three bars.
        let log = [run(1, Some(1), 0.8)];
        let stats = by_difficulty(&runs_of(&log, 1, Filter::default()));
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[0].difficulty, Difficulty::Easy);
        assert_eq!(stats[0].runs, 0);
        assert_eq!(stats[0].completion(), None, "unplayed is not 0 %");
        assert_eq!(stats[0].best_accuracy, None, "unplayed is not 0 % either");
        assert_eq!(stats[1].runs, 1);
        assert_eq!(stats[1].completion(), Some(1.0));
        assert_eq!(stats[1].best_accuracy, Some(0.8));
    }

    #[test]
    fn runs_before_the_roster_are_unattributed_not_lost() {
        let orphan = run(1, None, 0.6);
        let owned = run(2, Some(1), 0.8);
        let log = [orphan, owned];
        let loose = unattributed(&log, Filter::default());
        assert_eq!(loose.len(), 1);
        assert!((loose[0].accuracy - 0.6).abs() < 1e-9);
    }

    #[test]
    fn runs_come_back_oldest_first_whatever_order_the_log_is_in() {
        // The log is appended to, but a hand edit or a merged file
        // need not be sorted — and a progress line reads left to right.
        let log = [run(300, Some(1), 0.3), run(100, Some(1), 0.1)];
        let runs = runs_of(&log, 1, Filter::default());
        assert_eq!(runs[0].entry.started_ms, 100);
        assert_eq!(runs[1].entry.started_ms, 300);
    }

    #[test]
    fn a_player_who_finished_nothing_reports_no_accuracy_rather_than_zero() {
        // Two runs at Easy, both abandoned. "0 % best" would read as
        // "played and failed"; the truth is "never got to the end".
        let mut one = run(1, Some(1), 0.0);
        one.completed = false;
        one.difficulty = "easy".to_owned();
        let mut two = run(2, Some(1), 0.0);
        two.completed = false;
        two.difficulty = "easy".to_owned();
        let log = [one, two];
        let runs = runs_of(&log, 1, Filter::default());
        let summary = summarize(&runs);
        assert_eq!(summary.runs, 2, "the runs happened");
        assert_eq!(summary.mean_accuracy, None);
        assert_eq!(summary.best_accuracy, None);
        assert_eq!(summary.best_score, None);
        let easy = &by_difficulty(&runs)[0];
        assert_eq!(easy.runs, 2);
        assert_eq!(easy.completed, 0);
        assert_eq!(easy.best_accuracy, None);
        assert_eq!(easy.completion(), Some(0.0), "0 of 2 finished IS zero");
    }

    #[test]
    fn days_are_counted_the_way_the_export_stamps_them() {
        let log = [
            run(1_756_000_000_000, Some(1), 0.5),
            run(1_756_000_100_000, Some(1), 0.5),
            run(1_756_200_000_000, Some(1), 0.5),
        ];
        let runs = runs_of(&log, 1, Filter::default());
        let days = per_day(&runs);
        assert_eq!(days.len(), 2, "two calendar days");
        assert_eq!(days[0].1, 2);
        assert!(days[0].0 < days[1].0, "oldest day first");
    }
}
