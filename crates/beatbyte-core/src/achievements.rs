//! Achievements: one hundred of them, defined in one place.
//!
//! # How this stays extensible
//!
//! Two decisions carry the whole design.
//!
//! **The catalogue is data.** [`CATALOGUE`] is an array of
//! [`Achievement`], each one a title, a blurb and a [`Rule`] built
//! from a small closed vocabulary. Adding an achievement is adding an
//! entry — [`evaluate`] never learns a new name. Only a genuinely new
//! *kind* of condition touches the evaluator, and then it is one more
//! `Rule` or [`Test`] variant that every later entry can reuse.
//!
//! **Evaluation is pure and total, never incremental.** Every rule is
//! re-derived from the player's whole history on every pass. Nothing
//! is accumulated on disk except the moment each achievement was
//! unlocked. Three properties fall out of that for free:
//!
//! - an achievement added a year from now unlocks **retroactively**,
//!   from runs that were played before anybody thought of it;
//! - no counter can drift out of step with the log that feeds it, so
//!   there is nothing to migrate when the catalogue changes;
//! - a rule whose threshold is later raised cannot take an unlock
//!   back, because the store only ever remembers *when* — the caller
//!   merges, it does not recompute the past (see
//!   [`Unlocks::merge`]).
//!
//! # Rules the catalogue keeps
//!
//! *The autopilot is never counted.* It plays perfectly; every
//! achievement would be a lie. [`runs`] drops those lines and nothing
//! can switch that back on.
//!
//! *Assists are visible.* A run played with the tap assist, with No
//! Fail on, or at reduced speed is logged as such, and the harder
//! precision achievements exclude them. An achievement that cannot
//! tell an assisted run from an unassisted one devalues itself.
//!
//! *Nothing here rewards excess.* No achievement asks for a number of
//! runs in a day, a session longer than five songs, or money.
//!
//! # Calendar time is UTC
//!
//! [`crate::history::iso_utc`] is the only calendar arithmetic in
//! this workspace — there is no date crate, and the standard library
//! has no local time. Date and hour conditions therefore run on UTC.
//! For a day-long window such as Valentine's Day that is right for
//! twenty-three hours in twenty-four; for an hour window such as
//! "after midnight" it is shifted by the player's offset. Storing the
//! local offset per run would fix it and needs a dependency this
//! workspace does not carry.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::difficulty::Difficulty;
use crate::history::PlayEntry;
use crate::player::PlayerId;

/// What an achievement is about. Groups the overview screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    /// The first time you do anything.
    FirstSteps,
    /// Volume over a career.
    Endurance,
    /// Accuracy and timing.
    Precision,
    /// Streaks and clean runs.
    Combo,
    /// Climbing the difficulties.
    Difficulty,
    /// Breadth of the library played.
    Discovery,
    /// Star power.
    Hype,
    /// Habits: days, streaks, sittings.
    Ritual,
    /// Dates and times of day.
    Calendar,
    /// The strange corners.
    Oddities,
}

impl Category {
    /// Every category, in display order.
    pub const ALL: [Category; 10] = [
        Category::FirstSteps,
        Category::Endurance,
        Category::Precision,
        Category::Combo,
        Category::Difficulty,
        Category::Discovery,
        Category::Hype,
        Category::Ritual,
        Category::Calendar,
        Category::Oddities,
    ];

    /// The heading the overview shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Category::FirstSteps => "FIRST STEPS",
            Category::Endurance => "ENDURANCE",
            Category::Precision => "PRECISION",
            Category::Combo => "COMBO",
            Category::Difficulty => "DIFFICULTY",
            Category::Discovery => "DISCOVERY",
            Category::Hype => "HYPE",
            Category::Ritual => "RITUAL",
            Category::Calendar => "CALENDAR",
            Category::Oddities => "ODDITIES",
        }
    }
}

/// Roughly how hard, for sorting and for the badge colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    /// Reachable in the first session.
    Easy,
    /// A few evenings.
    Medium,
    /// Real practice.
    Hard,
    /// Seldom seen.
    Rare,
}

impl Tier {
    /// Every tier, easiest first.
    pub const ALL: [Tier; 4] = [Tier::Easy, Tier::Medium, Tier::Hard, Tier::Rare];

    /// The word the overview prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Tier::Easy => "EASY",
            Tier::Medium => "MEDIUM",
            Tier::Hard => "HARD",
            Tier::Rare => "RARE",
        }
    }
}

/// A predicate on ONE run. Every test in a list must hold for the
/// same run — that is what makes "95 % on Expert without practice" a
/// single achievement rather than three.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Test {
    /// The run reached the end of the song.
    Finished,
    /// Weighted accuracy at least this.
    MinAccuracy(f64),
    /// Best streak at least this.
    MinStreak(u32),
    /// Exactly this difficulty.
    IsDifficulty(Difficulty),
    /// This difficulty or harder.
    MinDifficulty(Difficulty),
    /// No note was missed.
    NoMiss,
    /// No strum hit nothing.
    NoOverstrum,
    /// Mean timing offset within this many milliseconds, either way.
    MaxDriftMs(f64),
    /// At least this many notes were judged.
    MinNotes(u32),
    /// At least this many overstrums (for the unflattering ones).
    MinOverstrums(u32),
    /// More overstrums than notes hit.
    MoreOverstrumsThanHits,
    /// The song is at least this long, in seconds.
    MinTrackS(f64),
    /// Practice speed and section loops were not used.
    NoPractice,
    /// The tap assist was off. Unknown (an older run) does not pass:
    /// an achievement may not credit a run that cannot answer.
    NoTapAssist,
    /// No Fail was off — the run could really have ended.
    NoSafetyNet,
    /// The run ended on an empty meter.
    DidFail,
    /// At least this many Hype activations.
    MinHype(u32),
    /// At least this many sustains held.
    MinSustainsHeld(u32),
    /// No sustain was let go early.
    NoSustainDropped,
    /// Share of judged notes that were perfect, at least this.
    MinPerfectShare(f64),
    /// The title contains one of these, case-insensitively.
    TitleAny(&'static [&'static str]),
    /// The title starts with this.
    TitleStartsWith(&'static str),
    /// The run started on this month and day (UTC).
    OnDate(u32, u32),
    /// The run started on one of these month/day pairs (UTC).
    OnDates(&'static [(u32, u32)]),
    /// The UTC hour is in `[from, to)`.
    HourIn(u32, u32),
    /// The UTC weekday, Monday = 0.
    OnWeekday(u32),
    /// The audio came from an imported file rather than a built-in.
    FromFile,
    /// Accuracy rounds to exactly this percentage, to one decimal.
    AccuracyIsExactly(f64),
}

/// A quantity summed over a career.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Runs started.
    Runs,
    /// Runs finished.
    Finished,
    /// Seconds played.
    Seconds,
    /// Notes judged hit (perfect + great + good).
    NotesHit,
    /// Notes judged perfect.
    Perfects,
    /// Points scored.
    Score,
    /// Hype activations.
    HypeActivations,
    /// Energy phrases completed.
    Phrases,
}

/// A thing there can be several distinct ones of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    /// Title + artist together.
    Song,
    /// Artist alone.
    Artist,
    /// Genre, where the log carries one.
    Genre,
    /// Calendar month (year + month).
    Month,
}

/// How an achievement is earned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rule {
    /// A career total reaches a target.
    Career {
        /// What is summed.
        metric: Metric,
        /// The target.
        target: f64,
    },
    /// One single run passes every test.
    OneRun(&'static [Test]),
    /// This many runs each pass every test.
    Runs {
        /// The tests each run must pass.
        tests: &'static [Test],
        /// How many such runs.
        target: usize,
    },
    /// This many distinct things, counted over runs that pass the
    /// tests.
    Distinct {
        /// What is counted.
        facet: Facet,
        /// The tests a run must pass to contribute.
        tests: &'static [Test],
        /// How many distinct.
        target: usize,
    },
    /// Played on this many distinct calendar days.
    Days(usize),
    /// Played on this many consecutive calendar days.
    DayStreak(usize),
    /// One song finished this many times.
    SameSong(usize),
    /// The same song played this many times back to back.
    SameSongInARow(usize),
    /// One song finished on every difficulty.
    AllDifficulties,
    /// This many runs with no gap longer than `gap_min` minutes.
    Sitting {
        /// How many runs.
        target: usize,
        /// The longest gap that still counts as one sitting.
        gap_min: f64,
    },
    /// A later run beats the player's own earlier best on the same
    /// song and difficulty by this much accuracy.
    Improved(f64),
    /// Two runs at least this many days apart.
    Comeback(f64),
    /// A song finished that the same player failed earlier.
    Redemption,
    /// Every one of these must hold.
    AllOf(&'static [Rule]),
}

/// One achievement.
#[derive(Debug, Clone, Copy)]
pub struct Achievement {
    /// Stable id. Never reused, never renamed — the store keys on it.
    pub id: &'static str,
    /// What the overview calls it.
    pub title: &'static str,
    /// What the player reads. For a hidden one this is shown only
    /// after it unlocks.
    pub blurb: &'static str,
    /// Which group it belongs to.
    pub category: Category,
    /// Roughly how hard.
    pub tier: Tier,
    /// Hidden achievements show as `???` until earned, and their
    /// condition is never spelled out before that.
    pub hidden: bool,
    /// How it is earned.
    pub rule: Rule,
}

/// Where an achievement stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// How far along, 0.0–1.0. Exactly 1.0 means earned.
    pub fraction: f64,
    /// The current count, where the rule is a count.
    pub have: f64,
    /// What the count is aiming for, where there is one.
    pub need: f64,
}

impl Progress {
    /// Nothing yet.
    const NONE: Progress = Progress {
        fraction: 0.0,
        have: 0.0,
        need: 1.0,
    };

    /// Earned.
    const DONE: Progress = Progress {
        fraction: 1.0,
        have: 1.0,
        need: 1.0,
    };

    /// A count against a target.
    fn counted(have: f64, need: f64) -> Progress {
        let need = need.max(1.0);
        Progress {
            fraction: (have / need).clamp(0.0, 1.0),
            have,
            need,
        }
    }

    /// Whether this counts as earned.
    #[must_use]
    pub fn earned(&self) -> bool {
        self.fraction >= 1.0
    }
}

/// When each achievement was unlocked, by id.
///
/// The ONLY thing persisted. Everything else is re-derived, which is
/// why a changed catalogue can never invalidate a player's past.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unlocks {
    /// Achievement id → unix milliseconds of the run that earned it.
    #[serde(default)]
    pub at: BTreeMap<String, u64>,
}

impl Unlocks {
    /// Fold newly earned achievements in, keeping the earliest date.
    ///
    /// Never removes: an achievement whose threshold is raised later,
    /// or which is dropped from the catalogue and brought back, stays
    /// earned. Returns the ids that were not there before — what the
    /// screen announces. Pure — tested.
    pub fn merge(&mut self, earned: &[(String, u64)]) -> Vec<String> {
        let mut fresh = Vec::new();
        for (id, when) in earned {
            if !self.at.contains_key(id) {
                self.at.insert(id.clone(), *when);
                fresh.push(id.clone());
            }
        }
        fresh
    }

    /// Whether this achievement is earned.
    #[must_use]
    pub fn has(&self, id: &str) -> bool {
        self.at.contains_key(id)
    }

    /// When it was earned.
    #[must_use]
    pub fn when(&self, id: &str) -> Option<u64> {
        self.at.get(id).copied()
    }

    /// How many of the catalogue are earned.
    #[must_use]
    pub fn count(&self) -> usize {
        CATALOGUE.iter().filter(|a| self.has(a.id)).count()
    }
}

/// One player's runs, oldest first, autopilot dropped.
///
/// The single gate every rule reads through: there is no filter that
/// puts the harness back in. Pure — tested.
#[must_use]
pub fn runs(entries: &[PlayEntry], player: PlayerId) -> Vec<&PlayEntry> {
    let mut mine: Vec<&PlayEntry> = entries
        .iter()
        .filter(|entry| !entry.autopilot && entry.part_of(player).is_some())
        .collect();
    mine.sort_by_key(|entry| entry.started_ms);
    mine
}

/// What one player has earned, and how far along everything else is.
///
/// Returns one [`Progress`] per catalogue entry, in catalogue order.
/// Pure — tested.
#[must_use]
pub fn evaluate(entries: &[PlayEntry], player: PlayerId) -> Vec<Progress> {
    let mine = runs(entries, player);
    CATALOGUE
        .iter()
        .map(|achievement| progress_of(&achievement.rule, &mine, player))
        .collect()
}

/// The achievements earned, each with the moment it was earned.
///
/// The moment is the start of the run that completed it — not now.
/// A catalogue added to after a year of play would otherwise stamp a
/// hundred unlocks with today's date and tell the player nothing.
/// Pure — tested.
#[must_use]
pub fn earned_at(entries: &[PlayEntry], player: PlayerId) -> Vec<(String, u64)> {
    let mine = runs(entries, player);
    let mut out = Vec::new();
    for achievement in CATALOGUE {
        if !progress_of(&achievement.rule, &mine, player).earned() {
            continue;
        }
        // Walk forward until the rule first holds: binary search
        // would be wrong, because not every rule is monotonic in the
        // prefix (a "same song three times in a row" can be broken by
        // a later run, but earning it once is enough).
        let mut when = mine.last().map_or(0, |entry| entry.started_ms);
        for cut in 1..=mine.len() {
            if progress_of(&achievement.rule, &mine[..cut], player).earned() {
                when = mine[cut - 1].started_ms;
                break;
            }
        }
        out.push((achievement.id.to_owned(), when));
    }
    out
}

/// A run's UTC calendar parts: year, month, day, hour, weekday
/// (Monday = 0). Derived from the one piece of calendar arithmetic
/// this workspace has. Pure — tested.
#[must_use]
pub fn calendar(started_ms: u64) -> (i64, u32, u32, u32, u32) {
    let stamp = crate::history::iso_utc(started_ms);
    let parse = |from: usize, to: usize| stamp[from..to].parse().unwrap_or(0);
    let year: i64 = stamp[..4].parse().unwrap_or(0);
    let (month, day, hour) = (parse(5, 7), parse(8, 10), parse(11, 13));
    // 1970-01-01 was a Thursday, which is index 3 counting from
    // Monday; days since the epoch therefore rotate from there.
    let weekday = ((started_ms / 86_400_000 + 3) % 7) as u32;
    (year, month, day, hour, weekday)
}

/// Whether one run passes every test. Pure — tested.
#[must_use]
pub fn passes(entry: &PlayEntry, player: PlayerId, tests: &[Test]) -> bool {
    let Some(part) = entry.part_of(player) else {
        return false;
    };
    let detail = part.detail;
    let (_, month, day, hour, weekday) = calendar(entry.started_ms);
    let hits =
        || detail.perfect.unwrap_or(0) + detail.great.unwrap_or(0) + detail.good.unwrap_or(0);
    tests.iter().all(|test| match *test {
        Test::Finished => entry.completed,
        Test::MinAccuracy(min) => part.accuracy >= min,
        Test::MinStreak(min) => detail.best_streak.is_some_and(|streak| streak >= min),
        Test::IsDifficulty(wanted) => Difficulty::from_id(&entry.difficulty) == Some(wanted),
        Test::MinDifficulty(min) => {
            Difficulty::from_id(&entry.difficulty).is_some_and(|d| d >= min)
        }
        // A run that did not record its judgments cannot claim a
        // clean one: absent is not zero.
        Test::NoMiss => detail.miss == Some(0),
        Test::NoOverstrum => detail.overstrums == Some(0),
        Test::MaxDriftMs(max) => detail.mean_offset_ms.is_some_and(|ms| ms.abs() <= max),
        Test::MinNotes(min) => detail.judged().is_some_and(|total| total >= min),
        Test::MinOverstrums(min) => detail.overstrums.is_some_and(|count| count >= min),
        Test::MoreOverstrumsThanHits => detail.overstrums.is_some_and(|over| over > hits()),
        Test::MinTrackS(min) => entry.track_s.is_some_and(|seconds| seconds >= min),
        Test::NoPractice => !entry.practice,
        Test::NoTapAssist => entry.tap_mode == Some(false),
        Test::NoSafetyNet => entry.no_fail == Some(false),
        Test::DidFail => detail.failed == Some(true),
        Test::MinHype(min) => detail.hype_activations.is_some_and(|count| count >= min),
        Test::MinSustainsHeld(min) => detail.sustains_held.is_some_and(|count| count >= min),
        Test::NoSustainDropped => detail.sustains_dropped == Some(0),
        Test::MinPerfectShare(min) => match (detail.perfect, detail.judged()) {
            (Some(perfect), Some(total)) if total > 0 => {
                f64::from(perfect) / f64::from(total) >= min
            }
            _ => false,
        },
        Test::TitleAny(words) => {
            let title = entry.title.to_lowercase();
            words.iter().any(|word| title.contains(word))
        }
        Test::TitleStartsWith(prefix) => entry.title.starts_with(prefix),
        Test::OnDate(m, d) => month == m && day == d,
        Test::OnDates(dates) => dates.iter().any(|(m, d)| month == *m && day == *d),
        Test::HourIn(from, to) => hour >= from && hour < to,
        Test::OnWeekday(wanted) => weekday == wanted,
        Test::FromFile => entry.source == "file",
        Test::AccuracyIsExactly(percent) => {
            ((part.accuracy * 1000.0).round() / 10.0 - percent).abs() < f64::EPSILON
        }
    })
}

/// How far along one rule is, over the runs given.
#[allow(clippy::too_many_lines)] // one arm per rule variant; splitting hides the table
fn progress_of(rule: &Rule, mine: &[&PlayEntry], player: PlayerId) -> Progress {
    match *rule {
        Rule::Career { metric, target } => {
            let have: f64 = mine
                .iter()
                .map(|entry| {
                    let part = entry.part_of(player);
                    let detail = part.as_ref().map(|p| p.detail);
                    match metric {
                        Metric::Runs => 1.0,
                        Metric::Finished => f64::from(u8::from(entry.completed)),
                        Metric::Seconds => entry.played_s,
                        Metric::NotesHit => detail.map_or(0.0, |d| {
                            f64::from(
                                d.perfect.unwrap_or(0) + d.great.unwrap_or(0) + d.good.unwrap_or(0),
                            )
                        }),
                        Metric::Perfects => {
                            detail.map_or(0.0, |d| f64::from(d.perfect.unwrap_or(0)))
                        }
                        Metric::Score => part.as_ref().map_or(0.0, |p| p.score as f64),
                        Metric::HypeActivations => {
                            detail.map_or(0.0, |d| f64::from(d.hype_activations.unwrap_or(0)))
                        }
                        Metric::Phrases => {
                            detail.map_or(0.0, |d| f64::from(d.phrases_completed.unwrap_or(0)))
                        }
                    }
                })
                .sum();
            Progress::counted(have, target)
        }
        Rule::OneRun(tests) => {
            if mine.iter().any(|entry| passes(entry, player, tests)) {
                Progress::DONE
            } else {
                Progress::NONE
            }
        }
        Rule::Runs { tests, target } => {
            let have = mine
                .iter()
                .filter(|entry| passes(entry, player, tests))
                .count();
            Progress::counted(have as f64, target as f64)
        }
        Rule::Distinct {
            facet,
            tests,
            target,
        } => {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for entry in mine.iter().filter(|e| passes(e, player, tests)) {
                match facet {
                    Facet::Song => {
                        seen.insert(format!("{}\u{1f}{}", entry.title, entry.artist));
                    }
                    Facet::Artist => {
                        seen.insert(entry.artist.to_lowercase());
                    }
                    Facet::Genre => {
                        if let Some(genre) = &entry.genre {
                            seen.insert(genre.to_lowercase());
                        }
                    }
                    Facet::Month => {
                        let (year, month, ..) = calendar(entry.started_ms);
                        seen.insert(format!("{year}-{month}"));
                    }
                }
            }
            Progress::counted(seen.len() as f64, target as f64)
        }
        Rule::Days(target) => {
            let days: BTreeSet<u64> = mine.iter().map(|e| e.started_ms / 86_400_000).collect();
            Progress::counted(days.len() as f64, target as f64)
        }
        Rule::DayStreak(target) => {
            let days: BTreeSet<u64> = mine.iter().map(|e| e.started_ms / 86_400_000).collect();
            let (mut best, mut run, mut previous) = (0_usize, 0_usize, None::<u64>);
            for day in days {
                // `day - 1` would overflow for a run stamped inside
                // the first day of the epoch, which every test
                // fixture with a small timestamp is.
                run = if previous.is_some_and(|before| before + 1 == day) {
                    run + 1
                } else {
                    1
                };
                best = best.max(run);
                previous = Some(day);
            }
            Progress::counted(best as f64, target as f64)
        }
        Rule::SameSong(target) => {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for entry in mine.iter().filter(|e| e.completed) {
                *counts
                    .entry(format!("{}\u{1f}{}", entry.title, entry.artist))
                    .or_insert(0) += 1;
            }
            Progress::counted(
                counts.values().copied().max().unwrap_or(0) as f64,
                target as f64,
            )
        }
        Rule::SameSongInARow(target) => {
            let (mut best, mut run, mut previous) = (0_usize, 0_usize, None::<String>);
            for entry in mine {
                let key = format!("{}\u{1f}{}", entry.title, entry.artist);
                run = if previous.as_ref() == Some(&key) {
                    run + 1
                } else {
                    1
                };
                best = best.max(run);
                previous = Some(key);
            }
            Progress::counted(best as f64, target as f64)
        }
        Rule::AllDifficulties => {
            let mut by_song: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for entry in mine.iter().filter(|e| e.completed) {
                by_song
                    .entry(format!("{}\u{1f}{}", entry.title, entry.artist))
                    .or_default()
                    .insert(entry.difficulty.clone());
            }
            let best = by_song.values().map(BTreeSet::len).max().unwrap_or(0);
            Progress::counted(best as f64, Difficulty::ALL.len() as f64)
        }
        Rule::Sitting { target, gap_min } => {
            let gap_ms = (gap_min * 60_000.0) as u64;
            let (mut best, mut run, mut previous) = (0_usize, 0_usize, None::<u64>);
            for entry in mine {
                run = match previous {
                    Some(before) if entry.started_ms.saturating_sub(before) <= gap_ms => run + 1,
                    _ => 1,
                };
                best = best.max(run);
                previous = Some(entry.started_ms);
            }
            Progress::counted(best as f64, target as f64)
        }
        Rule::Improved(by) => {
            let mut best: BTreeMap<String, f64> = BTreeMap::new();
            for entry in mine.iter().filter(|e| e.completed) {
                let Some(part) = entry.part_of(player) else {
                    continue;
                };
                let key = format!(
                    "{}\u{1f}{}\u{1f}{}",
                    entry.title, entry.artist, entry.difficulty
                );
                match best.get(&key) {
                    Some(previous) if part.accuracy >= previous + by => return Progress::DONE,
                    Some(previous) if part.accuracy <= *previous => {}
                    _ => {
                        best.insert(key, part.accuracy);
                    }
                }
            }
            Progress::NONE
        }
        Rule::Comeback(days) => {
            let gap_ms = (days * 86_400_000.0) as u64;
            let away = mine
                .windows(2)
                .any(|pair| pair[1].started_ms.saturating_sub(pair[0].started_ms) >= gap_ms);
            if away { Progress::DONE } else { Progress::NONE }
        }
        Rule::Redemption => {
            let mut failed: BTreeSet<String> = BTreeSet::new();
            for entry in mine {
                let key = format!("{}\u{1f}{}", entry.title, entry.artist);
                let detail = entry.part_of(player).map(|p| p.detail);
                if entry.completed && failed.contains(&key) {
                    return Progress::DONE;
                }
                if detail.is_some_and(|d| d.failed == Some(true)) {
                    failed.insert(key);
                }
            }
            Progress::NONE
        }
        Rule::AllOf(rules) => {
            let parts: Vec<Progress> = rules
                .iter()
                .map(|inner| progress_of(inner, mine, player))
                .collect();
            let fraction =
                parts.iter().map(|p| p.fraction).sum::<f64>() / parts.len().max(1) as f64;
            Progress {
                fraction,
                have: parts.iter().filter(|p| p.earned()).count() as f64,
                need: parts.len() as f64,
            }
        }
    }
}

// Word lists used by more than one entry, named so the catalogue
// reads as a table rather than as code.
/// Words a love song tends to carry, in the languages this library
/// holds. Deliberately generous: a near miss on Valentine's Day is a
/// better failure than a blank.
const LOVE_WORDS: &[&str] = &[
    "love", "heart", "kiss", "amour", "liebe", "romance", "darling", "baby", "honey", "sweet",
    "amore", "corazon", "beso",
];

/// The one hundred achievements.
///
/// Adding one is adding a row. The evaluator above never learns a
/// name; it only knows [`Rule`] and [`Test`]. Ids are stable and
/// prefixed by category so this array reads in the order the
/// overview groups it.
pub const CATALOGUE: &[Achievement] = &[
    // ---- FIRST STEPS -------------------------------------------
    Achievement {
        id: "first_run",
        title: "Plugged In",
        blurb: "Play your first run.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Runs,
            target: 1.0,
        },
    },
    Achievement {
        id: "first_second_song",
        title: "Second Verse",
        blurb: "Play two different songs.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Song,
            tests: &[],
            target: 2,
        },
    },
    Achievement {
        id: "first_finish",
        title: "All the Way Through",
        blurb: "Finish your first song.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished]),
    },
    Achievement {
        id: "first_medium",
        title: "Off the Training Wheels",
        blurb: "Play a Medium chart.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::IsDifficulty(Difficulty::Medium)]),
    },
    Achievement {
        id: "first_hard",
        title: "Turning It Up",
        blurb: "Play a Hard chart.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::IsDifficulty(Difficulty::Hard)]),
    },
    Achievement {
        id: "first_expert",
        title: "Into the Deep End",
        blurb: "Play an Expert chart.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::IsDifficulty(Difficulty::Expert)]),
    },
    Achievement {
        id: "first_80",
        title: "Eight Out of Ten",
        blurb: "Reach 80 % accuracy in a run.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinAccuracy(0.80)]),
    },
    Achievement {
        id: "first_fc",
        title: "Not One Missed",
        blurb: "Finish a run without a single miss.",
        category: Category::FirstSteps,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::NoMiss]),
    },
    Achievement {
        id: "first_file",
        title: "Your Own Music",
        blurb: "Play a song you imported yourself.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::FromFile]),
    },
    Achievement {
        id: "first_hype",
        title: "Star Power",
        blurb: "Activate Hype for the first time.",
        category: Category::FirstSteps,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::HypeActivations,
            target: 1.0,
        },
    },
    // ---- ENDURANCE ---------------------------------------------
    Achievement {
        id: "end_runs_10",
        title: "Warmed Up",
        blurb: "Start ten runs.",
        category: Category::Endurance,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Runs,
            target: 10.0,
        },
    },
    Achievement {
        id: "end_runs_100",
        title: "Hundred Runs",
        blurb: "Start a hundred runs.",
        category: Category::Endurance,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Runs,
            target: 100.0,
        },
    },
    Achievement {
        id: "end_runs_250",
        title: "Regular",
        blurb: "Start two hundred and fifty runs.",
        category: Category::Endurance,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Runs,
            target: 250.0,
        },
    },
    Achievement {
        id: "end_runs_500",
        title: "Five Hundred",
        blurb: "Start five hundred runs.",
        category: Category::Endurance,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Runs,
            target: 500.0,
        },
    },
    Achievement {
        id: "end_finished_10",
        title: "Ten Down",
        blurb: "Finish ten songs.",
        category: Category::Endurance,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Finished,
            target: 10.0,
        },
    },
    Achievement {
        id: "end_finished_50",
        title: "Fifty Down",
        blurb: "Finish fifty songs.",
        category: Category::Endurance,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Finished,
            target: 50.0,
        },
    },
    Achievement {
        id: "end_finished_100",
        title: "Century Club",
        blurb: "Finish a hundred songs.",
        category: Category::Endurance,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Finished,
            target: 100.0,
        },
    },
    Achievement {
        id: "end_finished_250",
        title: "Two Fifty",
        blurb: "Finish two hundred and fifty songs.",
        category: Category::Endurance,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Finished,
            target: 250.0,
        },
    },
    Achievement {
        id: "end_hours_1",
        title: "An Hour In",
        blurb: "Play for an hour in total.",
        category: Category::Endurance,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Seconds,
            target: 3_600.0,
        },
    },
    Achievement {
        id: "end_hours_5",
        title: "Five Hours",
        blurb: "Play for five hours in total.",
        category: Category::Endurance,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Seconds,
            target: 18_000.0,
        },
    },
    Achievement {
        id: "end_hours_20",
        title: "Twenty Hours",
        blurb: "Play for twenty hours in total.",
        category: Category::Endurance,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Seconds,
            target: 72_000.0,
        },
    },
    Achievement {
        id: "end_hours_50",
        title: "Fifty Hours",
        blurb: "Play for fifty hours in total.",
        category: Category::Endurance,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Seconds,
            target: 180_000.0,
        },
    },
    Achievement {
        id: "end_score_1m",
        title: "Millionaire",
        blurb: "Score a million points in total.",
        category: Category::Endurance,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Score,
            target: 1_000_000.0,
        },
    },
    // ---- PRECISION ---------------------------------------------
    Achievement {
        id: "prec_70",
        title: "Seven Out of Ten",
        blurb: "Reach 70 % accuracy in a run.",
        category: Category::Precision,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinAccuracy(0.70)]),
    },
    Achievement {
        id: "prec_90",
        title: "Nine Out of Ten",
        blurb: "Reach 90 % accuracy without practice mode.",
        category: Category::Precision,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinAccuracy(0.90), Test::NoPractice]),
    },
    Achievement {
        id: "prec_95",
        title: "Ninety-Five",
        blurb: "Reach 95 % accuracy without practice mode.",
        category: Category::Precision,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinAccuracy(0.95), Test::NoPractice]),
    },
    Achievement {
        id: "prec_98",
        title: "Near Perfect",
        blurb: "Reach 98 % accuracy without practice mode.",
        category: Category::Precision,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinAccuracy(0.98), Test::NoPractice]),
    },
    Achievement {
        id: "prec_100",
        title: "Perfection",
        blurb: "Finish a chart of a hundred notes or more at 100 %.",
        category: Category::Precision,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::MinAccuracy(1.0),
            Test::MinNotes(100),
            Test::NoPractice,
        ]),
    },
    Achievement {
        id: "prec_drift_5",
        title: "In the Pocket",
        blurb: "Finish a run with a mean timing drift inside 5 ms.",
        category: Category::Precision,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::MaxDriftMs(5.0)]),
    },
    Achievement {
        id: "prec_drift_2",
        title: "Metronome",
        blurb: "Finish a run with a mean timing drift inside 2 ms.",
        category: Category::Precision,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::MaxDriftMs(2.0)]),
    },
    Achievement {
        id: "prec_no_overstrum",
        title: "Clean Hands",
        blurb: "Finish a run of a hundred notes with no overstrum.",
        category: Category::Precision,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::NoOverstrum, Test::MinNotes(100)]),
    },
    Achievement {
        id: "prec_perfect_share",
        title: "Mostly Perfect",
        blurb: "Finish a run where four notes in five were PERFECT.",
        category: Category::Precision,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::MinPerfectShare(0.80),
            Test::MinNotes(100),
        ]),
    },
    Achievement {
        id: "prec_perfects_1k",
        title: "A Thousand Perfects",
        blurb: "Hit a thousand PERFECT notes in your career.",
        category: Category::Precision,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Perfects,
            target: 1_000.0,
        },
    },
    Achievement {
        id: "prec_perfects_10k",
        title: "Ten Thousand Perfects",
        blurb: "Hit ten thousand PERFECT notes in your career.",
        category: Category::Precision,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Perfects,
            target: 10_000.0,
        },
    },
    Achievement {
        id: "prec_perfects_50k",
        title: "Fifty Thousand Perfects",
        blurb: "Hit fifty thousand PERFECT notes in your career.",
        category: Category::Precision,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Perfects,
            target: 50_000.0,
        },
    },
    Achievement {
        id: "prec_improve_20",
        title: "Twenty Points Better",
        blurb: "Beat your own best on a song by twenty accuracy points.",
        category: Category::Precision,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Improved(0.20),
    },
    // ---- COMBO -------------------------------------------------
    Achievement {
        id: "combo_50",
        title: "Fifty in a Row",
        blurb: "Reach a streak of fifty notes.",
        category: Category::Combo,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinStreak(50)]),
    },
    Achievement {
        id: "combo_100",
        title: "Century",
        blurb: "Reach a streak of a hundred notes.",
        category: Category::Combo,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinStreak(100)]),
    },
    Achievement {
        id: "combo_250",
        title: "Locked In",
        blurb: "Reach a streak of two hundred and fifty notes.",
        category: Category::Combo,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinStreak(250)]),
    },
    Achievement {
        id: "combo_500",
        title: "Unbroken",
        blurb: "Reach a streak of five hundred notes.",
        category: Category::Combo,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinStreak(500)]),
    },
    Achievement {
        id: "combo_1000",
        title: "Thousand Yard Stare",
        blurb: "Reach a streak of a thousand notes.",
        category: Category::Combo,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinStreak(1000)]),
    },
    Achievement {
        id: "combo_fc_medium",
        title: "Flawless Medium",
        blurb: "Full combo a Medium chart: no miss, no overstrum.",
        category: Category::Combo,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::IsDifficulty(Difficulty::Medium),
            Test::NoMiss,
            Test::NoOverstrum,
            Test::NoPractice,
        ]),
    },
    Achievement {
        id: "combo_fc_hard",
        title: "Flawless Hard",
        blurb: "Full combo a Hard chart: no miss, no overstrum.",
        category: Category::Combo,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::IsDifficulty(Difficulty::Hard),
            Test::NoMiss,
            Test::NoOverstrum,
            Test::NoPractice,
        ]),
    },
    Achievement {
        id: "combo_fc_expert",
        title: "Flawless Expert",
        blurb: "Full combo an Expert chart, with no assists at all.",
        category: Category::Combo,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::IsDifficulty(Difficulty::Expert),
            Test::NoMiss,
            Test::NoOverstrum,
            Test::NoPractice,
            Test::NoTapAssist,
        ]),
    },
    Achievement {
        id: "combo_fc_long",
        title: "Endurance Run",
        blurb: "Full combo a song over five minutes long.",
        category: Category::Combo,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::NoMiss,
            Test::NoOverstrum,
            Test::MinTrackS(300.0),
            Test::NoPractice,
        ]),
    },
    Achievement {
        id: "combo_notes_100k",
        title: "Six Figures",
        blurb: "Hit a hundred thousand notes in your career.",
        category: Category::Combo,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::NotesHit,
            target: 100_000.0,
        },
    },
    // ---- DIFFICULTY --------------------------------------------
    Achievement {
        id: "diff_easy",
        title: "Easy Does It",
        blurb: "Finish a song on Easy.",
        category: Category::Difficulty,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::IsDifficulty(Difficulty::Easy)]),
    },
    Achievement {
        id: "diff_medium",
        title: "Stepping Up",
        blurb: "Finish a song on Medium.",
        category: Category::Difficulty,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::IsDifficulty(Difficulty::Medium)]),
    },
    Achievement {
        id: "diff_hard",
        title: "Hard Cleared",
        blurb: "Finish a song on Hard.",
        category: Category::Difficulty,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::IsDifficulty(Difficulty::Hard)]),
    },
    Achievement {
        id: "diff_expert",
        title: "Expert Cleared",
        blurb: "Finish a song on Expert.",
        category: Category::Difficulty,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::IsDifficulty(Difficulty::Expert)]),
    },
    Achievement {
        id: "diff_hard_90",
        title: "Hard, Mastered",
        blurb: "Reach 90 % on Hard without practice mode.",
        category: Category::Difficulty,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::IsDifficulty(Difficulty::Hard),
            Test::MinAccuracy(0.90),
            Test::NoPractice,
        ]),
    },
    Achievement {
        id: "diff_expert_90",
        title: "Expert, Mastered",
        blurb: "Reach 90 % on Expert without practice mode.",
        category: Category::Difficulty,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::IsDifficulty(Difficulty::Expert),
            Test::MinAccuracy(0.90),
            Test::NoPractice,
        ]),
    },
    Achievement {
        id: "diff_expert_10",
        title: "Expert Regular",
        blurb: "Finish ten Expert runs.",
        category: Category::Difficulty,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::Runs {
            tests: &[Test::Finished, Test::IsDifficulty(Difficulty::Expert)],
            target: 10,
        },
    },
    Achievement {
        id: "diff_expert_nonet",
        title: "No Safety Net",
        blurb: "Finish an Expert chart with No Fail switched off.",
        category: Category::Difficulty,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::IsDifficulty(Difficulty::Expert),
            Test::NoSafetyNet,
        ]),
    },
    Achievement {
        id: "diff_no_assist",
        title: "Strum It Yourself",
        blurb: "Finish a Hard chart with the tap assist off.",
        category: Category::Difficulty,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::MinDifficulty(Difficulty::Hard),
            Test::NoTapAssist,
        ]),
    },
    Achievement {
        id: "diff_all_four",
        title: "Four Ways Up",
        blurb: "Finish one song on all four difficulties.",
        category: Category::Difficulty,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::AllDifficulties,
    },
    // ---- DISCOVERY ---------------------------------------------
    Achievement {
        id: "disc_songs_10",
        title: "Getting Around",
        blurb: "Finish ten different songs.",
        category: Category::Discovery,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Song,
            tests: &[Test::Finished],
            target: 10,
        },
    },
    Achievement {
        id: "disc_songs_25",
        title: "Well Travelled",
        blurb: "Finish twenty-five different songs.",
        category: Category::Discovery,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Song,
            tests: &[Test::Finished],
            target: 25,
        },
    },
    Achievement {
        id: "disc_songs_50",
        title: "Deep Catalogue",
        blurb: "Finish fifty different songs.",
        category: Category::Discovery,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Song,
            tests: &[Test::Finished],
            target: 50,
        },
    },
    Achievement {
        id: "disc_songs_100",
        title: "Hundred Songs",
        blurb: "Finish a hundred different songs.",
        category: Category::Discovery,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Song,
            tests: &[Test::Finished],
            target: 100,
        },
    },
    Achievement {
        id: "disc_artists_10",
        title: "Ten Bands",
        blurb: "Finish songs by ten different artists.",
        category: Category::Discovery,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Artist,
            tests: &[Test::Finished],
            target: 10,
        },
    },
    Achievement {
        id: "disc_artists_25",
        title: "Festival Line-Up",
        blurb: "Finish songs by twenty-five different artists.",
        category: Category::Discovery,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Artist,
            tests: &[Test::Finished],
            target: 25,
        },
    },
    Achievement {
        id: "disc_artists_50",
        title: "Record Collection",
        blurb: "Finish songs by fifty different artists.",
        category: Category::Discovery,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Artist,
            tests: &[Test::Finished],
            target: 50,
        },
    },
    Achievement {
        id: "disc_genres_3",
        title: "Genre Hopper",
        blurb: "Finish songs in three different genres.",
        category: Category::Discovery,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Genre,
            tests: &[Test::Finished],
            target: 3,
        },
    },
    Achievement {
        id: "disc_genres_5",
        title: "Omnivore",
        blurb: "Finish songs in five different genres.",
        category: Category::Discovery,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Genre,
            tests: &[Test::Finished],
            target: 5,
        },
    },
    Achievement {
        id: "disc_genres_8",
        title: "No Bad Music",
        blurb: "Finish songs in eight different genres.",
        category: Category::Discovery,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Genre,
            tests: &[Test::Finished],
            target: 8,
        },
    },
    Achievement {
        id: "disc_long_song",
        title: "The Long Haul",
        blurb: "Finish a song over seven minutes long.",
        category: Category::Discovery,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::MinTrackS(420.0)]),
    },
    // ---- HYPE --------------------------------------------------
    Achievement {
        id: "hype_10",
        title: "Crowd Pleaser",
        blurb: "Activate Hype ten times.",
        category: Category::Hype,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::HypeActivations,
            target: 10.0,
        },
    },
    Achievement {
        id: "hype_100",
        title: "Showman",
        blurb: "Activate Hype a hundred times.",
        category: Category::Hype,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::HypeActivations,
            target: 100.0,
        },
    },
    Achievement {
        id: "hype_500",
        title: "Headliner",
        blurb: "Activate Hype five hundred times.",
        category: Category::Hype,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::HypeActivations,
            target: 500.0,
        },
    },
    Achievement {
        id: "hype_3_in_run",
        title: "Triple Threat",
        blurb: "Activate Hype three times in one song.",
        category: Category::Hype,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinHype(3)]),
    },
    Achievement {
        id: "hype_5_in_run",
        title: "Pyrotechnics",
        blurb: "Activate Hype five times in one song.",
        category: Category::Hype,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::OneRun(&[Test::MinHype(5)]),
    },
    Achievement {
        id: "hype_phrases_100",
        title: "Phrase Hunter",
        blurb: "Complete a hundred energy phrases.",
        category: Category::Hype,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Career {
            metric: Metric::Phrases,
            target: 100.0,
        },
    },
    Achievement {
        id: "hype_sustain_clean",
        title: "Hold That Note",
        blurb: "Finish a song holding every one of its sustains to the end.",
        category: Category::Hype,
        tier: Tier::Hard,
        hidden: true,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::MinSustainsHeld(10),
            Test::NoSustainDropped,
        ]),
    },
    // ---- RITUAL ------------------------------------------------
    Achievement {
        id: "ritual_days_5",
        title: "Five Days",
        blurb: "Play on five different days.",
        category: Category::Ritual,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Days(5),
    },
    Achievement {
        id: "ritual_days_25",
        title: "Twenty-Five Days",
        blurb: "Play on twenty-five different days.",
        category: Category::Ritual,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Days(25),
    },
    Achievement {
        id: "ritual_days_100",
        title: "Hundred Days",
        blurb: "Play on a hundred different days.",
        category: Category::Ritual,
        tier: Tier::Hard,
        hidden: false,
        rule: Rule::Days(100),
    },
    Achievement {
        id: "ritual_streak_3",
        title: "Three Days Running",
        blurb: "Play three days in a row.",
        category: Category::Ritual,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::DayStreak(3),
    },
    Achievement {
        id: "ritual_streak_7",
        title: "A Full Week",
        blurb: "Play seven days in a row.",
        category: Category::Ritual,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::DayStreak(7),
    },
    Achievement {
        id: "ritual_streak_30",
        title: "A Month Straight",
        blurb: "Play thirty days in a row.",
        category: Category::Ritual,
        tier: Tier::Rare,
        hidden: false,
        rule: Rule::DayStreak(30),
    },
    Achievement {
        id: "ritual_months_3",
        title: "Three Months",
        blurb: "Play in three different calendar months.",
        category: Category::Ritual,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::Distinct {
            facet: Facet::Month,
            tests: &[],
            target: 3,
        },
    },
    Achievement {
        id: "ritual_session_5",
        title: "One More Song",
        blurb: "Play five runs in one sitting.",
        category: Category::Ritual,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::Sitting {
            target: 5,
            gap_min: 30.0,
        },
    },
    Achievement {
        id: "ritual_comeback",
        title: "Back Again",
        blurb: "Come back to the guitar after a month away.",
        category: Category::Ritual,
        tier: Tier::Medium,
        hidden: true,
        rule: Rule::Comeback(30.0),
    },
    // ---- CALENDAR ----------------------------------------------
    Achievement {
        id: "cal_night_owl",
        title: "Night Owl",
        blurb: "Finish a song between midnight and four in the morning.",
        category: Category::Calendar,
        tier: Tier::Easy,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::HourIn(0, 4)]),
    },
    Achievement {
        id: "cal_early_bird",
        title: "Dawn Patrol",
        blurb: "Finish a song between five and eight in the morning.",
        category: Category::Calendar,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::HourIn(5, 8)]),
    },
    Achievement {
        id: "cal_friday_night",
        title: "Friday Night Lights",
        blurb: "Finish a song on a Friday evening.",
        category: Category::Calendar,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::Finished, Test::OnWeekday(4), Test::HourIn(20, 24)]),
    },
    Achievement {
        id: "cal_weekend",
        title: "Weekend Warrior",
        blurb: "Finish a song on a Saturday and on a Sunday.",
        category: Category::Calendar,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::AllOf(&[
            Rule::OneRun(&[Test::Finished, Test::OnWeekday(5)]),
            Rule::OneRun(&[Test::Finished, Test::OnWeekday(6)]),
        ]),
    },
    Achievement {
        id: "cal_new_year",
        title: "Auld Lang Syne",
        blurb: "Play on New Year's Eve or New Year's Day.",
        category: Category::Calendar,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::OnDates(&[(12, 31), (1, 1)])]),
    },
    Achievement {
        id: "cal_halloween",
        title: "Trick or Riff",
        blurb: "Play on the thirty-first of October.",
        category: Category::Calendar,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::OnDate(10, 31)]),
    },
    Achievement {
        id: "cal_christmas",
        title: "Silent Night",
        blurb: "Play on Christmas Eve or Christmas Day.",
        category: Category::Calendar,
        tier: Tier::Medium,
        hidden: false,
        rule: Rule::OneRun(&[Test::OnDates(&[(12, 24), (12, 25)])]),
    },
    Achievement {
        id: "cal_valentine",
        title: "Love Song",
        blurb: "Finish a song about love, on Valentine's Day.",
        category: Category::Calendar,
        tier: Tier::Rare,
        hidden: true,
        rule: Rule::OneRun(&[
            Test::Finished,
            Test::OnDate(2, 14),
            Test::TitleAny(LOVE_WORDS),
        ]),
    },
    Achievement {
        id: "cal_leap_day",
        title: "One Day in Four Years",
        blurb: "Play on the twenty-ninth of February.",
        category: Category::Calendar,
        tier: Tier::Rare,
        hidden: true,
        rule: Rule::OneRun(&[Test::OnDate(2, 29)]),
    },
    // ---- ODDITIES ----------------------------------------------
    Achievement {
        id: "odd_guitar_study",
        title: "The Study",
        blurb: "Finish one of the [Guitar Study] charts.",
        category: Category::Oddities,
        tier: Tier::Easy,
        hidden: true,
        rule: Rule::OneRun(&[Test::Finished, Test::TitleStartsWith("[Guitar Study]")]),
    },
    Achievement {
        id: "odd_failed",
        title: "That Went Badly",
        blurb: "Let the crowd turn on you completely.",
        category: Category::Oddities,
        tier: Tier::Easy,
        hidden: true,
        rule: Rule::OneRun(&[Test::DidFail]),
    },
    Achievement {
        id: "odd_three_in_row",
        title: "Again. Again. Again.",
        blurb: "Play the same song three times back to back.",
        category: Category::Oddities,
        tier: Tier::Easy,
        hidden: true,
        rule: Rule::SameSongInARow(3),
    },
    Achievement {
        id: "odd_same_song_10",
        title: "On Repeat",
        blurb: "Finish the same song ten times.",
        category: Category::Oddities,
        tier: Tier::Medium,
        hidden: true,
        rule: Rule::SameSong(10),
    },
    Achievement {
        id: "odd_more_overstrums",
        title: "Air Guitar",
        blurb: "Strum at more thin air than notes in a single run.",
        category: Category::Oddities,
        tier: Tier::Medium,
        hidden: true,
        rule: Rule::OneRun(&[Test::MoreOverstrumsThanHits, Test::MinOverstrums(20)]),
    },
    Achievement {
        id: "odd_redemption",
        title: "Redemption",
        blurb: "Finish a song that once beat you.",
        category: Category::Oddities,
        tier: Tier::Medium,
        hidden: true,
        rule: Rule::Redemption,
    },
    Achievement {
        id: "odd_dead_on",
        title: "Dead On",
        blurb: "Finish a run with a mean timing drift inside one millisecond.",
        category: Category::Oddities,
        tier: Tier::Rare,
        hidden: true,
        rule: Rule::OneRun(&[Test::Finished, Test::MaxDriftMs(1.0)]),
    },
    Achievement {
        id: "odd_exactly_half",
        title: "Exactly Half",
        blurb: "Finish a run at exactly fifty per cent accuracy.",
        category: Category::Oddities,
        tier: Tier::Rare,
        hidden: true,
        rule: Rule::OneRun(&[Test::Finished, Test::AccuracyIsExactly(50.0)]),
    },
];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::history::{RunDetail, RunPart};

    fn run(started_ms: u64, accuracy: f64, completed: bool) -> PlayEntry {
        PlayEntry {
            title: "Africa".to_owned(),
            artist: "Toto".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms,
            played_s: 120.0,
            track_s: Some(120.0),
            completed,
            players: 1,
            practice: false,
            autopilot: false,
            score: 1000,
            accuracy,
            source: "file".to_owned(),
            player: Some(1),
            detail: RunDetail::default(),
            co_players: Vec::new(),
            chart_hash: None,
            genre: None,
            tap_mode: Some(false),
            no_fail: Some(false),
            speed_percent: Some(100),
        }
    }

    #[test]
    fn the_catalogue_holds_exactly_one_hundred_with_unique_ids() {
        assert_eq!(CATALOGUE.len(), 100, "the catalogue is not a hundred");
        let ids: BTreeSet<&str> = CATALOGUE.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), CATALOGUE.len(), "two entries share an id");
        let titles: BTreeSet<&str> = CATALOGUE.iter().map(|a| a.title).collect();
        assert_eq!(titles.len(), CATALOGUE.len(), "two entries share a title");
    }

    #[test]
    fn every_entry_is_legible_and_filed_somewhere() {
        for entry in CATALOGUE {
            assert!(
                !entry.id.is_empty() && !entry.title.is_empty(),
                "{}",
                entry.id
            );
            assert!(
                entry.blurb.len() > 10 && entry.blurb.ends_with('.'),
                "{} has no readable blurb",
                entry.id
            );
            // The id carries its category, which is what makes the
            // array readable and the overview groupable.
            let prefix = entry.id.split('_').next().unwrap();
            assert!(
                !prefix.is_empty() && prefix.len() <= 6,
                "{} has no category prefix",
                entry.id
            );
        }
    }

    #[test]
    fn every_category_and_tier_is_actually_used() {
        // A category nobody is in is a filter that shows an empty
        // list, which reads as a broken screen.
        for category in Category::ALL {
            assert!(
                CATALOGUE.iter().any(|a| a.category == category),
                "{} has no achievements",
                category.label()
            );
        }
        for tier in Tier::ALL {
            assert!(CATALOGUE.iter().any(|a| a.tier == tier), "{:?}", tier);
        }
    }

    #[test]
    fn the_catalogue_keeps_some_secrets_but_not_too_many() {
        let hidden = CATALOGUE.iter().filter(|a| a.hidden).count();
        assert!(
            (5..=25).contains(&hidden),
            "{hidden} hidden: a catalogue that is mostly secret has no goals, \
             and one with no secrets has nothing to discover"
        );
    }

    #[test]
    fn the_autopilot_earns_nothing_at_all() {
        // It plays perfectly. If it counted, a harness run would hand
        // out the hardest achievements in the catalogue.
        let mut bot = run(1, 1.0, true);
        bot.autopilot = true;
        bot.detail = RunDetail {
            best_streak: Some(5000),
            perfect: Some(5000),
            great: Some(0),
            good: Some(0),
            miss: Some(0),
            overstrums: Some(0),
            mean_offset_ms: Some(0.0),
            hype_activations: Some(50),
            phrases_completed: Some(500),
            sustains_held: Some(100),
            sustains_dropped: Some(0),
            failed: Some(false),
        };
        assert!(runs(&[bot.clone()], 1).is_empty());
        assert!(
            earned_at(&[bot], 1).is_empty(),
            "the autopilot earned achievements"
        );
    }

    #[test]
    fn a_first_run_earns_the_first_steps_and_nothing_deep() {
        let log = [run(1_756_000_000_000, 0.85, true)];
        let earned: BTreeSet<String> = earned_at(&log, 1).into_iter().map(|(id, _)| id).collect();
        assert!(earned.contains("first_run"), "{earned:?}");
        assert!(earned.contains("first_finish"));
        assert!(earned.contains("first_80"));
        assert!(
            !earned.contains("first_second_song"),
            "one run is not two songs"
        );
        assert!(earned.contains("diff_medium"));
        // …and nothing that needs a career.
        assert!(!earned.contains("end_runs_100"));
        assert!(!earned.contains("disc_songs_10"));
    }

    #[test]
    fn an_achievement_is_dated_to_the_run_that_earned_it_not_to_now() {
        // Otherwise a catalogue added after a year of play stamps a
        // hundred unlocks with today and tells the player nothing.
        let log = [
            run(1_000, 0.5, true),
            run(2_000, 0.9, true),
            run(3_000, 0.95, true),
        ];
        let earned: BTreeMap<String, u64> = earned_at(&log, 1).into_iter().collect();
        assert_eq!(earned.get("first_run"), Some(&1_000));
        // 80 % was first reached by the SECOND run.
        assert_eq!(earned.get("first_80"), Some(&2_000));
    }

    #[test]
    fn a_missing_signal_never_grants_an_achievement() {
        // An old run recorded no judgments. "No miss" must not be
        // true just because the miss count is absent.
        let old = run(1, 1.0, true);
        assert_eq!(old.detail.miss, None);
        assert!(!passes(&old, 1, &[Test::NoMiss]));
        assert!(!passes(&old, 1, &[Test::NoOverstrum]));
        assert!(!passes(&old, 1, &[Test::MinHype(1)]));
        assert!(!passes(&old, 1, &[Test::MaxDriftMs(100.0)]));
    }

    #[test]
    fn an_unlock_is_never_taken_back() {
        // A threshold raised later, or an entry removed and restored,
        // must not cost a player something they earned.
        let mut unlocks = Unlocks::default();
        let fresh = unlocks.merge(&[("first_run".to_owned(), 100)]);
        assert_eq!(fresh, vec!["first_run".to_owned()]);
        // A second pass with a LATER date keeps the first date and
        // announces nothing.
        let again = unlocks.merge(&[("first_run".to_owned(), 999)]);
        assert!(again.is_empty(), "announced twice");
        assert_eq!(unlocks.when("first_run"), Some(100), "the date moved");
        assert!(unlocks.has("first_run"));
    }

    #[test]
    fn progress_is_reported_for_the_road_as_well_as_the_arrival() {
        // A visible achievement that only says "not yet" is a wall;
        // the fraction is what makes it a goal.
        let log: Vec<PlayEntry> = (0..5).map(|n| run(1_000 + n * 1_000, 0.5, true)).collect();
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "end_runs_10")
            .unwrap();
        let progress = evaluate(&log, 1)[at];
        assert!((progress.have - 5.0).abs() < 1e-9);
        assert!((progress.need - 10.0).abs() < 1e-9);
        assert!((progress.fraction - 0.5).abs() < 1e-9);
        assert!(!progress.earned());
    }

    #[test]
    fn a_day_streak_counts_consecutive_days_not_runs() {
        let day = 86_400_000_u64;
        // Three runs on ONE day is not a three-day streak.
        let same_day: Vec<PlayEntry> = (0..3)
            .map(|n| run(day * 10 + n * 1000, 0.5, true))
            .collect();
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "ritual_streak_3")
            .unwrap();
        assert!(!evaluate(&same_day, 1)[at].earned());
        // Three consecutive days is.
        let three_days: Vec<PlayEntry> = (0..3).map(|n| run(day * (10 + n), 0.5, true)).collect();
        assert!(evaluate(&three_days, 1)[at].earned());
        // A gap breaks it.
        let broken = [
            run(day * 10, 0.5, true),
            run(day * 12, 0.5, true),
            run(day * 13, 0.5, true),
        ];
        assert!(!evaluate(&broken, 1)[at].earned());
        // Playing TWICE on one of the days must not break the
        // streak — it is the commonest shape a real week has, and
        // counting runs instead of days silently loses it.
        let twice_on_tuesday = [
            run(day * 10, 0.5, true),
            run(day * 11, 0.5, true),
            run(day * 11 + 3_600_000, 0.5, true),
            run(day * 12, 0.5, true),
        ];
        assert!(
            evaluate(&twice_on_tuesday, 1)[at].earned(),
            "a second run on one day ended the streak"
        );
    }

    #[test]
    fn the_calendar_reads_the_same_day_the_export_stamps() {
        // 2026-02-14, a Saturday, 12:00 UTC.
        let (year, month, day, hour, weekday) = calendar(1_771_070_400_000);
        assert_eq!((year, month, day), (2026, 2, 14));
        assert_eq!(hour, 12);
        assert_eq!(weekday, 5, "Saturday is index 5 counting from Monday");
        assert!(crate::history::iso_utc(1_771_070_400_000).starts_with("2026-02-14"));
    }

    #[test]
    fn valentine_needs_both_the_day_and_a_love_song() {
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "cal_valentine")
            .unwrap();
        // The right day, the wrong song.
        let wrong_song = [run(1_771_070_400_000, 0.9, true)];
        assert!(!evaluate(&wrong_song, 1)[at].earned());
        // The right song, the wrong day (one day later).
        let mut late = run(1_771_070_400_000 + 86_400_000, 0.9, true);
        late.title = "Love Shack".to_owned();
        assert!(!evaluate(&[late], 1)[at].earned());
        // Both.
        let mut right = run(1_771_070_400_000, 0.9, true);
        right.title = "Whole Lotta Love".to_owned();
        assert!(evaluate(&[right], 1)[at].earned());
    }

    #[test]
    fn a_sitting_is_broken_by_a_long_gap() {
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "ritual_session_5")
            .unwrap();
        // Five runs ten minutes apart: one sitting.
        let close: Vec<PlayEntry> = (0..5)
            .map(|n| run(1_000_000 + n * 600_000, 0.5, true))
            .collect();
        assert!(evaluate(&close, 1)[at].earned());
        // Five runs an hour apart: five sittings.
        let apart: Vec<PlayEntry> = (0..5)
            .map(|n| run(1_000_000 + n * 3_600_000, 0.5, true))
            .collect();
        assert!(!evaluate(&apart, 1)[at].earned());
    }

    #[test]
    fn redemption_needs_the_defeat_to_come_first() {
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "odd_redemption")
            .unwrap();
        let mut lost = run(1_000, 0.2, false);
        lost.detail.failed = Some(true);
        let won = run(2_000, 0.9, true);
        assert!(evaluate(&[lost.clone(), won.clone()], 1)[at].earned());
        // The other way round is just a win followed by a loss.
        let mut later_loss = lost;
        later_loss.started_ms = 3_000;
        assert!(!evaluate(&[won, later_loss], 1)[at].earned());
    }

    #[test]
    fn improving_means_beating_your_own_earlier_best() {
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "prec_improve_20")
            .unwrap();
        // 60 % then 85 % on the same song and difficulty: +25 points.
        let better = [run(1_000, 0.60, true), run(2_000, 0.85, true)];
        assert!(evaluate(&better, 1)[at].earned());
        // The same jump on a DIFFERENT song proves nothing about you.
        let mut other = run(2_000, 0.85, true);
        other.title = "Something Else".to_owned();
        assert!(!evaluate(&[run(1_000, 0.60, true), other], 1)[at].earned());
        // Getting worse is not improving.
        assert!(!evaluate(&[run(1_000, 0.85, true), run(2_000, 0.60, true)], 1)[at].earned());
    }

    #[test]
    fn an_assist_cannot_buy_the_hardest_achievements() {
        let at = CATALOGUE
            .iter()
            .position(|a| a.id == "combo_fc_expert")
            .unwrap();
        let clean = |tap: Option<bool>, practice: bool| {
            let mut entry = run(1_000, 1.0, true);
            entry.difficulty = "expert".to_owned();
            entry.tap_mode = tap;
            entry.practice = practice;
            entry.detail = RunDetail {
                miss: Some(0),
                overstrums: Some(0),
                ..RunDetail::default()
            };
            entry
        };
        assert!(evaluate(&[clean(Some(false), false)], 1)[at].earned());
        assert!(
            !evaluate(&[clean(Some(true), false)], 1)[at].earned(),
            "the tap assist bought a Flawless Expert"
        );
        assert!(
            !evaluate(&[clean(Some(false), true)], 1)[at].earned(),
            "practice mode bought a Flawless Expert"
        );
        assert!(
            !evaluate(&[clean(None, false)], 1)[at].earned(),
            "a run that cannot say whether it was assisted was credited"
        );
    }

    #[test]
    fn a_co_player_earns_their_own_achievements() {
        // Being player two is playing; the numbers live in co_players.
        let mut entry = run(1_000, 0.95, true);
        entry.players = 2;
        entry.co_players.push(RunPart {
            player: Some(2),
            score: 10,
            accuracy: 0.40,
            detail: RunDetail::default(),
        });
        let first: BTreeSet<String> = earned_at(std::slice::from_ref(&entry), 1)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let second: BTreeSet<String> = earned_at(&[entry], 2)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(first.contains("first_80"), "slot one missed their own 95 %");
        assert!(!second.contains("first_80"), "slot two got slot one's run");
        assert!(second.contains("first_finish"), "slot two did finish it");
    }

    #[test]
    fn no_two_achievements_ask_for_the_same_thing() {
        // A duplicate condition is filler wearing a second name. It
        // is also how a tautological test hides: `HasPlayer` was one
        // — `part_of` only ever returns a part whose player is set,
        // so the test could not be false and the achievement was
        // "you have played a run" under a second title.
        for (a, first) in CATALOGUE.iter().enumerate() {
            for second in &CATALOGUE[a + 1..] {
                assert!(
                    first.rule != second.rule,
                    "`{}` and `{}` ask for exactly the same thing",
                    first.id,
                    second.id
                );
            }
        }
    }

    #[test]
    fn every_test_variant_is_used_by_some_achievement() {
        // A predicate nobody uses is dead weight in `passes`, and the
        // place a dead one is most likely to survive is right after
        // the achievement that was its only caller was rewritten.
        fn note(rule: &Rule, seen: &mut BTreeSet<&'static str>) {
            let tests: &[Test] = match rule {
                Rule::OneRun(tests) | Rule::Runs { tests, .. } | Rule::Distinct { tests, .. } => {
                    tests
                }
                Rule::AllOf(inner) => {
                    for rule in *inner {
                        note(rule, seen);
                    }
                    &[]
                }
                _ => &[],
            };
            for test in tests {
                seen.insert(match test {
                    Test::Finished => "finished",
                    Test::MinAccuracy(_) => "min_accuracy",
                    Test::MinStreak(_) => "min_streak",
                    Test::IsDifficulty(_) => "is_difficulty",
                    Test::MinDifficulty(_) => "min_difficulty",
                    Test::NoMiss => "no_miss",
                    Test::NoOverstrum => "no_overstrum",
                    Test::MaxDriftMs(_) => "max_drift",
                    Test::MinNotes(_) => "min_notes",
                    Test::MinOverstrums(_) => "min_overstrums",
                    Test::MoreOverstrumsThanHits => "more_over_than_hits",
                    Test::MinTrackS(_) => "min_track_s",
                    Test::NoPractice => "no_practice",
                    Test::NoTapAssist => "no_tap_assist",
                    Test::NoSafetyNet => "no_safety_net",
                    Test::DidFail => "did_fail",
                    Test::MinHype(_) => "min_hype",
                    Test::MinSustainsHeld(_) => "min_sustains_held",
                    Test::NoSustainDropped => "no_sustain_dropped",
                    Test::MinPerfectShare(_) => "min_perfect_share",
                    Test::TitleAny(_) => "title_any",
                    Test::TitleStartsWith(_) => "title_starts_with",
                    Test::OnDate(..) => "on_date",
                    Test::OnDates(_) => "on_dates",
                    Test::HourIn(..) => "hour_in",
                    Test::OnWeekday(_) => "on_weekday",
                    Test::FromFile => "from_file",
                    Test::AccuracyIsExactly(_) => "accuracy_exactly",
                });
            }
        }
        let mut seen = BTreeSet::new();
        for entry in CATALOGUE {
            note(&entry.rule, &mut seen);
        }
        assert_eq!(seen.len(), 28, "unused test variants: {seen:?}");
    }

    #[test]
    fn every_rule_variant_is_reachable_from_the_catalogue() {
        // A rule nobody uses is dead weight in the evaluator; this
        // keeps the vocabulary honest as the catalogue grows.
        let mut seen = BTreeSet::new();
        fn note(rule: &Rule, seen: &mut BTreeSet<&'static str>) {
            let name = match rule {
                Rule::Career { .. } => "career",
                Rule::OneRun(_) => "one_run",
                Rule::Runs { .. } => "runs",
                Rule::Distinct { .. } => "distinct",
                Rule::Days(_) => "days",
                Rule::DayStreak(_) => "day_streak",
                Rule::SameSong(_) => "same_song",
                Rule::SameSongInARow(_) => "same_song_row",
                Rule::AllDifficulties => "all_difficulties",
                Rule::Sitting { .. } => "sitting",
                Rule::Improved(_) => "improved",
                Rule::Comeback(_) => "comeback",
                Rule::Redemption => "redemption",
                Rule::AllOf(inner) => {
                    for rule in *inner {
                        note(rule, seen);
                    }
                    "all_of"
                }
            };
            seen.insert(name);
        }
        for entry in CATALOGUE {
            note(&entry.rule, &mut seen);
        }
        assert_eq!(seen.len(), 14, "unused rule variants: {seen:?}");
    }
}
