//! The taste test: the same half minute of a song, twice, on two
//! chart versions, without being told which is which.
//!
//! A rating of a whole run measures a lot of things at once — the
//! song, the day, how awake you are — and two ratings from two
//! evenings are barely comparable. The fast way to tell whether a
//! change to the generator helped is to hear the SAME passage
//! immediately twice and say which one was better, and the only
//! honest way to do that is blind: knowing which one is "the new
//! one" decides the answer before the music starts. The house
//! already works this way elsewhere (the disco controller's blind
//! white-effect test).
//!
//! What is deliberately NOT here: a new verdict format. "The second
//! one was better" is exactly [`beatbyte_core::telemetry::NoteLine::Versus`]
//! against the other version's hash, which `beatbyte-cli review`
//! already tallies.

use std::path::Path;

use bevy::prelude::*;

use beatbyte_chart::{ChartFile, chart_hash};
use beatbyte_core::Difficulty;

/// How long one side of the test plays, seconds. Long enough for a
/// phrase to establish itself, short enough that the second side is
/// heard against a fresh memory of the first.
pub const WINDOW_S: f64 = 30.0;

/// How much music runs before the window's first note, so the notes
/// scroll in instead of landing on the receptors. BOTH sides begin
/// here: a side that got more approach room than the other would be
/// comparing the run-up, not the charting.
pub const LEAD_IN_S: f64 = 1.5;

/// Where a side's music actually starts, given its window.
/// Pure — tested.
#[must_use]
pub fn side_start(window: (f64, f64)) -> f64 {
    (window.0 - LEAD_IN_S).max(0.0)
}

/// Which side the player preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The one that played first.
    First,
    /// The one that played second.
    Second,
    /// No difference worth naming.
    Same,
}

/// The window this song is tested on.
///
/// ⚠️ A delegate, not a copy: the rule lives in the chart crate so
/// the offline tool can report what changed in exactly the thirty
/// seconds this plays. Two copies would measure two windows.
#[must_use]
pub fn window_for(chart: &ChartFile, difficulty: Difficulty, length_s: f64) -> (f64, f64) {
    chart.preview_window(difficulty, length_s)
}

/// Which version plays first, decided by a seed rather than by which
/// one is "new".
///
/// Deterministic on purpose: the same seed replays the same order, so
/// a run can be reconstructed from the log. Pure — tested.
#[must_use]
pub fn blind_order(seed: u64) -> [usize; 2] {
    // splitmix, the workspace's one source of seeded randomness.
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    if (z ^ (z >> 31)) & 1 == 0 {
        [0, 1]
    } else {
        [1, 0]
    }
}

/// The version a choice actually names: `(winner, loser)` as indices
/// into the versions as they were GIVEN, not as they were played.
///
/// This is the whole point of the blind test and the one place its
/// answer can be turned around: the player says "the second one",
/// which is `order[1]`. Pure — tested.
#[must_use]
pub fn preferred(choice: Choice, order: [usize; 2]) -> Option<(usize, usize)> {
    match choice {
        Choice::First => Some((order[0], order[1])),
        Choice::Second => Some((order[1], order[0])),
        Choice::Same => None,
    }
}

/// What the reveal says once the verdict is recorded.
#[must_use]
pub fn reveal(order: [usize; 2], names: [&str; 2]) -> String {
    format!(
        "you heard {} first, then {}",
        names[order[0]], names[order[1]]
    )
}

/// The two versions a folder offers for a blind test: the active one
/// and its immediate neighbour — the version it was almost certainly
/// derived from (a redesign writes the next number and points at it).
///
/// Returned oldest first, so the caller's indices mean something
/// before [`blind_order`] shuffles them. `None` when the folder holds
/// only one chart: there is nothing to compare. Pure — tested.
#[must_use]
pub fn pair_for(active: &str, files: &[String]) -> Option<[String; 2]> {
    // The base chart is version 1; `chart.vN.json` is version N.
    let number = |name: &str| -> Option<u32> {
        if name == beatbyte_chart::versions::BASE_CHART {
            Some(1)
        } else {
            beatbyte_chart::versions::version_number(name)
        }
    };
    let here = number(active)?;
    let mut others: Vec<(u32, &String)> = files
        .iter()
        .filter(|f| f.as_str() != active)
        .filter_map(|f| number(f).map(|n| (n, f)))
        .collect();
    others.sort_by_key(|(n, _)| *n);
    // The nearest version BELOW the active one is its parent; with
    // nothing below (the base is active) the nearest above will do.
    let neighbour = others
        .iter()
        .rev()
        .find(|(n, _)| *n < here)
        .or_else(|| others.iter().find(|(n, _)| *n > here))?;
    let other = neighbour.1.clone();
    if neighbour.0 < here {
        Some([other, active.to_owned()])
    } else {
        Some([active.to_owned(), other])
    }
}

/// A chart holding only what sounds inside `(from, to)`.
///
/// The whole test is one window heard twice, and a session that ends
/// on its own is worth more than one stopped by a timer: cropped, the
/// track simply runs out, and every downstream rule — the end of the
/// run, the results snapshot, the telemetry total — works unchanged.
/// A sustain that starts inside the window keeps its full length;
/// cutting it would change the very thing being judged. Pure — tested.
#[must_use]
pub fn crop(chart: &ChartFile, (from, to): (f64, f64)) -> ChartFile {
    let mut cropped = chart.clone();
    for def in &mut cropped.charts {
        def.notes.retain(|note| note.time >= from && note.time < to);
        def.phrases
            .retain(|phrase| phrase.end > from && phrase.start < to);
    }
    cropped
}

/// The verdict line for a blind test, from the point of view of the
/// version the session log NAMES.
///
/// The log's header is written from the loaded song on the way out of
/// gameplay, and by then the chart on it is the one that played
/// SECOND — so `order[1]` is the subject and `order[0]` is the
/// `parent` the verdict is against. Getting this backwards would
/// credit every blind test to the losing chart, silently and
/// plausibly, which is why the mapping lives in one pinned function
/// rather than inline at the keypress. Pure — tested.
#[must_use]
pub fn versus_line(
    choice: Choice,
    order: [usize; 2],
    hashes: [&str; 2],
) -> Option<(String, String)> {
    let (winner, _) = preferred(choice, order)?;
    let verdict = if winner == order[1] {
        "better"
    } else {
        "worse"
    };
    Some((verdict.to_owned(), hashes[order[0]].to_owned()))
}

/// One side of a taste test: a chart version and what to call it.
#[derive(Debug, Clone)]
pub struct TasteVersion {
    /// The file name of the version, e.g. `chart.v5.json`.
    pub name: String,
    /// Its content hash, for the verdict line.
    pub hash: String,
    /// The chart this side plays, already cropped to the window.
    pub chart: ChartFile,
}

/// A taste test in progress.
#[derive(Resource, Debug, Clone)]
pub struct TasteTest {
    /// The two versions, as given.
    pub versions: [TasteVersion; 2],
    /// Which one plays first.
    pub order: [usize; 2],
    /// How many sides have been played.
    pub played: usize,
    /// The window both sides are tested on.
    pub window: (f64, f64),
}

impl TasteTest {
    /// Whether another side follows the one playing now.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.played < self.order.len()
    }

    /// The version index playing now, if the test is still running.
    #[must_use]
    pub fn current(&self) -> Option<usize> {
        self.order.get(self.played).copied()
    }

    /// Finish the side playing now and return the next one's version
    /// index, or `None` when both sides have been heard.
    pub fn advance(&mut self) -> Option<usize> {
        self.played += 1;
        self.current()
    }
}

/// Build the blind test for the song whose chart sits at
/// `chart_path`: its folder's active version and the neighbour it
/// came from, both cropped to one window, in an order this run's
/// seed decides.
///
/// Reads the folder rather than the browser's scan, because the scan
/// sees one chart per song and the test is about the ones it does
/// not show.
pub fn build(chart_path: &Path, difficulty: Difficulty, seed: u64) -> Result<TasteTest, String> {
    let dir = chart_path
        .parent()
        .ok_or_else(|| "that chart has no folder".to_owned())?;
    let names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read the song folder: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let active = chart_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "that chart has no file name".to_owned())?;
    let pair = pair_for(&active, &names)
        .ok_or_else(|| format!("\"{active}\" is the only chart here — nothing to compare"))?;
    let mut loaded = Vec::new();
    for name in &pair {
        let chart = beatbyte_chart::load_chart_file(&dir.join(name))
            .map_err(|error| format!("cannot load {name}: {error}"))?;
        if !chart.charts.iter().any(|c| c.difficulty == difficulty) {
            return Err(format!("{name} has no {difficulty} chart"));
        }
        loaded.push((name.clone(), chart));
    }
    // The window comes from the ACTIVE version — the one the player
    // would otherwise hear — so the test is about the passage the
    // song is normally shown by.
    let window = window_for(&loaded[1].1, difficulty, WINDOW_S);
    let versions: Vec<TasteVersion> = loaded
        .into_iter()
        .map(|(name, chart)| TasteVersion {
            name,
            hash: chart_hash(&chart),
            chart: crop(&chart, window),
        })
        .collect();
    let [first, second] = <[TasteVersion; 2]>::try_from(versions)
        .map_err(|_| "expected exactly two versions".to_owned())?;
    if first.hash == second.hash {
        return Err("both versions are the same chart — nothing to hear".to_owned());
    }
    Ok(TasteTest {
        versions: [first, second],
        order: blind_order(seed),
        played: 0,
        window,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::schema::{ChartDef, ChartNote, SongMeta};

    fn chart(preview: Option<f64>, notes: &[f64]) -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Test".to_owned(),
                artist: "Unit".to_owned(),
                audio: "t.wav".to_owned(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: preview,
                duration_s: Some(200.0),
                genre: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Medium,
                lanes: 5,
                notes: notes
                    .iter()
                    .map(|t| ChartNote {
                        time: *t,
                        lane: 0,
                        len: 0.0,
                        hopo: false,
                    })
                    .collect(),
                phrases: Vec::new(),
            }],
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        }
    }

    #[test]
    fn the_preview_anchor_is_where_the_song_shows_itself() {
        let file = chart(Some(63.7), &[1.0, 2.0]);
        assert_eq!(window_for(&file, Difficulty::Medium, 30.0), (63.7, 93.7));
    }

    #[test]
    fn without_an_anchor_the_busiest_half_minute_wins() {
        // Two notes at the start, five packed later: a test on the
        // opening would compare two nearly empty passages.
        let mut times = vec![0.0, 1.0];
        times.extend([100.0, 101.0, 102.0, 103.0, 104.0]);
        let file = chart(None, &times);
        let (from, to) = window_for(&file, Difficulty::Medium, 30.0);
        assert!(
            (from - 100.0).abs() < 1e-9,
            "starts at the dense part: {from}"
        );
        assert!((to - 130.0).abs() < 1e-9);
        // A chart with no notes at all still yields a usable window.
        assert_eq!(
            window_for(&chart(None, &[]), Difficulty::Medium, 30.0),
            (0.0, 30.0)
        );
    }

    #[test]
    fn a_nonsense_anchor_does_not_decide_the_window() {
        for bad in [f64::NAN, f64::INFINITY, -5.0] {
            let file = chart(Some(bad), &[10.0, 11.0, 12.0]);
            let (from, _) = window_for(&file, Difficulty::Medium, 30.0);
            assert!(from.is_finite() && from >= 0.0, "{bad} gave {from}");
        }
    }

    #[test]
    fn both_sides_start_with_the_same_run_up() {
        assert!((side_start((63.7, 93.7)) - 62.2).abs() < 1e-9);
        // A window at the very top of the song cannot run up into
        // negative time, and must not ask the player to.
        assert_eq!(side_start((0.0, 30.0)), 0.0);
        assert_eq!(side_start((1.0, 31.0)), 0.0);
    }

    #[test]
    fn the_order_is_seeded_and_goes_both_ways() {
        let orders: Vec<[usize; 2]> = (0..64).map(blind_order).collect();
        assert!(orders.contains(&[0, 1]), "some seeds put the first first");
        assert!(orders.contains(&[1, 0]), "and some put it second");
        assert_eq!(blind_order(7), blind_order(7), "the same seed replays");
        for order in orders {
            assert_ne!(order[0], order[1], "both sides play exactly once");
        }
    }

    #[test]
    fn a_choice_names_the_version_that_played_then_not_the_slot() {
        // The player says "the second one". With the order reversed,
        // the second one IS version 0 — getting this backwards would
        // credit every verdict to the wrong chart, silently.
        assert_eq!(preferred(Choice::Second, [1, 0]), Some((0, 1)));
        assert_eq!(preferred(Choice::Second, [0, 1]), Some((1, 0)));
        assert_eq!(preferred(Choice::First, [1, 0]), Some((1, 0)));
        assert_eq!(preferred(Choice::Same, [0, 1]), None, "a tie names nobody");
    }

    #[test]
    fn the_reveal_names_them_in_the_order_they_played() {
        assert_eq!(
            reveal([1, 0], ["chart.v4.json", "chart.v5.json"]),
            "you heard chart.v5.json first, then chart.v4.json"
        );
    }

    #[test]
    fn a_test_knows_how_far_it_has_come() {
        let version = |name: &str| TasteVersion {
            name: name.to_owned(),
            hash: "h".to_owned(),
            chart: chart(None, &[]),
        };
        let mut test = TasteTest {
            versions: [version("a"), version("b")],
            order: [1, 0],
            played: 0,
            window: (10.0, 40.0),
        };
        assert_eq!(test.current(), Some(1), "the order decides, not the slot");
        assert!(test.has_next());
        test.played = 1;
        assert_eq!(test.current(), Some(0));
        test.played = 2;
        assert!(!test.has_next(), "both sides played");
        assert_eq!(test.current(), None);
        test.played = 1;
        assert_eq!(test.advance(), None, "advancing off the end names nobody");
    }

    #[test]
    fn the_pair_is_the_active_version_and_its_neighbour() {
        let files =
            |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_owned()).collect() };
        let all = files(&["chart.json", "chart.v2.json", "chart.v3.json", "notes.txt"]);
        assert_eq!(
            pair_for("chart.v3.json", &all),
            Some(["chart.v2.json".to_owned(), "chart.v3.json".to_owned()]),
            "the parent is the version below"
        );
        // The base is active (a revert): the nearest version ABOVE is
        // the only thing left to compare against.
        assert_eq!(
            pair_for("chart.json", &all),
            Some(["chart.json".to_owned(), "chart.v2.json".to_owned()])
        );
        // A gap in the numbering is not a hole in the pair.
        assert_eq!(
            pair_for("chart.v7.json", &files(&["chart.json", "chart.v7.json"])),
            Some(["chart.json".to_owned(), "chart.v7.json".to_owned()])
        );
        assert_eq!(
            pair_for("chart.json", &files(&["chart.json"])),
            None,
            "one chart is nothing to compare"
        );
    }

    #[test]
    fn a_cropped_chart_holds_only_the_window() {
        let mut file = chart(None, &[1.0, 64.0, 70.0, 93.6, 93.7, 120.0]);
        file.charts[0].phrases = vec![
            beatbyte_chart::schema::ChartPhrase {
                start: 0.0,
                end: 2.0,
            },
            beatbyte_chart::schema::ChartPhrase {
                start: 65.0,
                end: 68.0,
            },
        ];
        let cut = crop(&file, (63.7, 93.7));
        let times: Vec<f64> = cut.charts[0].notes.iter().map(|n| n.time).collect();
        assert_eq!(
            times,
            vec![64.0, 70.0, 93.6],
            "the window's end is exclusive"
        );
        assert_eq!(
            cut.charts[0].phrases.len(),
            1,
            "phrases outside are dropped"
        );
        // The song's own facts are untouched: the audio is the whole
        // file, and the clock's length bound reads this.
        assert_eq!(cut.song.duration_s, file.song.duration_s);
    }

    #[test]
    fn a_sustain_that_starts_inside_keeps_its_length() {
        let mut file = chart(None, &[10.0]);
        file.charts[0].notes[0].len = 9.0;
        let cut = crop(&file, (5.0, 12.0));
        assert_eq!(cut.charts[0].notes.len(), 1);
        assert!(
            (cut.charts[0].notes[0].len - 9.0).abs() < 1e-9,
            "cutting it would change what is being judged"
        );
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-taste-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write(dir: &Path, name: &str, chart: &ChartFile) {
        let json = serde_json::to_string(chart).expect("serialize");
        std::fs::write(dir.join(name), json).expect("write");
    }

    #[test]
    fn a_built_test_pairs_two_different_charts_on_one_window() {
        let dir = folder("pair");
        write(&dir, "chart.json", &chart(Some(60.0), &[61.0, 62.0, 200.0]));
        write(
            &dir,
            "chart.v2.json",
            &chart(Some(60.0), &[61.5, 63.0, 200.0]),
        );
        let test = build(&dir.join("chart.v2.json"), Difficulty::Medium, 1).expect("built");
        assert_eq!(test.window, (60.0, 90.0), "the anchor decides the window");
        assert_eq!(test.versions[0].name, "chart.json");
        assert_eq!(test.versions[1].name, "chart.v2.json");
        assert_ne!(test.versions[0].hash, test.versions[1].hash);
        // Both sides are cropped to the same window: the note at 200 s
        // belongs to neither.
        for version in &test.versions {
            let times: Vec<f64> = version.chart.charts[0]
                .notes
                .iter()
                .map(|n| n.time)
                .collect();
            assert!(times.iter().all(|t| *t < 90.0), "{times:?} left the window");
            assert_eq!(times.len(), 2);
        }
        assert_eq!(test.played, 0);
        assert!(test.has_next());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_with_nothing_to_compare_refuses() {
        let dir = folder("lone");
        write(&dir, "chart.json", &chart(Some(10.0), &[11.0]));
        let error = build(&dir.join("chart.json"), Difficulty::Medium, 1).expect_err("refused");
        assert!(error.contains("only chart"), "{error}");
        // Two files whose charts are IDENTICAL are not a blind test
        // either: the verdict would be about the day, not the charts.
        let same = chart(Some(10.0), &[11.0]);
        write(&dir, "chart.v2.json", &same);
        write(&dir, "chart.json", &same);
        let error = build(&dir.join("chart.v2.json"), Difficulty::Medium, 1).expect_err("refused");
        assert!(error.contains("same chart"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_without_the_chosen_difficulty_refuses() {
        let dir = folder("diff");
        write(&dir, "chart.json", &chart(Some(10.0), &[11.0]));
        write(&dir, "chart.v2.json", &chart(Some(10.0), &[12.0]));
        let error = build(&dir.join("chart.v2.json"), Difficulty::Expert, 1).expect_err("refused");
        assert!(
            error.contains("Expert") || error.contains("expert"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_verdict_names_the_version_the_log_names() {
        // The log's subject is the chart that played SECOND. Preferring
        // the second side therefore means "better"; preferring the
        // first means the subject was the worse of the two.
        assert_eq!(
            versus_line(Choice::Second, [0, 1], ["h0", "h1"]),
            Some(("better".to_owned(), "h0".to_owned()))
        );
        assert_eq!(
            versus_line(Choice::First, [0, 1], ["h0", "h1"]),
            Some(("worse".to_owned(), "h0".to_owned()))
        );
        // Reversed order: the parent is still whatever played first.
        assert_eq!(
            versus_line(Choice::Second, [1, 0], ["h0", "h1"]),
            Some(("better".to_owned(), "h1".to_owned()))
        );
        assert_eq!(
            versus_line(Choice::First, [1, 0], ["h0", "h1"]),
            Some(("worse".to_owned(), "h1".to_owned()))
        );
        assert_eq!(versus_line(Choice::Same, [0, 1], ["h0", "h1"]), None);
    }
}
