//! Classic: the rules the early guitar games played by, one
//! ingredient at a time.
//!
//! Every function here **re-flags or re-shapes an existing chart**
//! rather than generating one. That is the point: the blind test
//! ([`crate::study`]'s twin, the browser's `T`) can only answer a
//! question about ONE variable, so an ingredient changes exactly one
//! thing and leaves the notes, the frets, the times, the sustains
//! and the phrases where they were.
//!
//! ⚠️ Only numbers and rules from the research come in here, never a
//! chart or a note of anybody else's music — the rule CLAUDE.md
//! states about assets. A threshold is a fact about an engine; a
//! chart is a work.
//!
//! The ingredients are meant to be switched on one at a time, in the
//! order they were measured to matter, and each has its own flag in
//! the [`Recipe`]:
//!
//! - **`hopo`** (K1) — the hammer-on threshold in beats, not seconds;
//! - **`strum`** (K2) — a strum under the wrong fret waits
//!   [`STRUM_GRACE_MS`] for the fret to follow. The only ingredient
//!   that touches no note: it is a judgment rule the chart carries
//!   ([`crate::schema::Rules`]).

use std::path::{Path, PathBuf};

use crate::grid::BeatGrid;
use crate::schema::{ChartDef, ChartFile, ChartNote, Provenance};
use crate::{Severity, chart_hash, context, load_chart_file, save_chart_file, twin, versions};
use beatbyte_core::Difficulty;

/// Who a classic chart's provenance names.
pub const DESIGNER: &str = "classic";

/// The kind of twin this module writes.
pub const KIND: twin::Kind = twin::Kind::Classic;

/// The HOPO threshold the early games shipped: 170 ticks of the 480
/// that make a beat.
///
/// ⚠️ **Beats, not seconds, and exclusive.** That is the whole
/// difference this ingredient makes. Our generator asks whether the
/// gap is at most 0.22 s (Expert) or 0.26 s (Hard), which is a
/// question about the CLOCK: past about 136 BPM on Expert and 115 on
/// Hard, plain eighth notes fall under it and become hammer-ons, and
/// the library's Hard charts are 39 % HOPO because of it. Asking in
/// beats instead, a plain eighth is 0.5 beats and never qualifies;
/// an eighth-note triplet is 0.333 and always does. So: triplets and
/// faster are hammered, straight eighths are strummed, at every
/// tempo.
pub const HOPO_BEATS: f64 = 170.0 / 480.0;

/// How long a strum under the wrong fret waits for the fret, in the
/// early games' engine: about 60 ms, then the note is judged.
///
/// ⚠️ A rule, not a window. The hit window stays ±100 ms either way;
/// what this changes is the ORDER the hand may move in — pick first,
/// fret a moment later, the natural motion on a fast change. Measured
/// before it was built, read-only, from the recorded sessions: on
/// Hard, 18 of 139 overstrums showed exactly that pattern. Four
/// sessions out of 144 recorded the individual presses, so the number
/// is thin; the blind test decides, not it.
pub const STRUM_GRACE_MS: u16 = 60;

/// The beat ruler a chart carries, lifted out so the flags can be
/// rewritten while it is read.
///
/// ⚠️ It reads the TRACKED grid where there is one. A chart's
/// constant `bpm` is not its grid — the analysis has tracked a
/// time-varying one since format v0.14.30, and on a live recording
/// the two drift more than a second apart by the end. A beat-
/// relative rule asked against the wrong ruler is a rule about
/// nothing.
#[derive(Debug, Clone)]
pub struct Beats {
    grid: Option<BeatGrid>,
    /// The constant fallback, for a chart from before the grid.
    constant_s: f64,
}

impl Beats {
    /// The ruler this chart means.
    #[must_use]
    pub fn of(chart: &ChartFile) -> Beats {
        Beats {
            grid: chart.grid.clone(),
            constant_s: if chart.song.bpm > 0.0 {
                60.0 / chart.song.bpm
            } else {
                0.0
            },
        }
    }

    /// A ruler of one fixed beat length, for tests and for a chart
    /// that has no grid.
    #[must_use]
    pub const fn constant(beat_s: f64) -> Beats {
        Beats {
            grid: None,
            constant_s: beat_s,
        }
    }

    /// How long a beat is at `time_s`. Zero when nothing says.
    #[must_use]
    pub fn at(&self, time_s: f64) -> f64 {
        self.grid
            .as_ref()
            .and_then(|grid| grid.beat_length_at(time_s))
            .filter(|beat| beat.is_finite() && *beat > 0.0)
            .unwrap_or(self.constant_s)
    }
}

/// One note event: the indices of the notes struck together.
///
/// Grouped by [`crate::convert::CHORD_EPSILON_S`], the same window
/// the engine uses when it turns a chart into a track — so "chord"
/// here means what the player's hand has to do, not what the file
/// happens to round to.
fn events(notes: &[ChartNote]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|a, b| {
        notes[*a]
            .time
            .partial_cmp(&notes[*b].time)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(notes[*a].lane.cmp(&notes[*b].lane))
    });
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut anchor = f64::NEG_INFINITY;
    for index in order {
        let time = notes[index].time;
        match out.last_mut() {
            Some(last) if (time - anchor).abs() <= crate::convert::CHORD_EPSILON_S => {
                last.push(index);
            }
            _ => {
                anchor = time;
                out.push(vec![index]);
            }
        }
    }
    out
}

/// Re-flag one difficulty's hammer-ons under the classic rule.
///
/// Pure, and it touches **nothing but the `hopo` flags** — a pin
/// says so, because the blind test this feeds can only compare one
/// variable at a time. Returns how many notes changed.
///
/// The rule, as the early games ran it:
/// - never the first event;
/// - never a chord, and never a note inside one;
/// - a single note whose gap to the event before it is **under**
///   [`HOPO_BEATS`] of the local beat;
/// - whose fret is not one the hand already had down — a different
///   lane from the previous single note, and not a member of the
///   previous chord.
pub fn reflag_hopos(notes: &mut [ChartNote], beats: &Beats) -> usize {
    let grouped = events(notes);
    let mut changed = 0usize;
    let mut previous: Option<(f64, Vec<u8>)> = None;
    for group in grouped {
        let time = notes[group[0]].time;
        let lanes: Vec<u8> = group.iter().map(|i| notes[*i].lane).collect();
        let hopo = match &previous {
            // The first event is a strum: there is no chain to
            // continue, and nothing to pull off from.
            None => false,
            Some((before, held)) => {
                let beat = beats.at(time);
                let window = HOPO_BEATS * beat;
                // Chords are strummed in every one of these games.
                group.len() == 1
                    && beat > 0.0
                    && time - before < window
                    && !held.contains(&lanes[0])
            }
        };
        for index in &group {
            if notes[*index].hopo != hopo {
                notes[*index].hopo = hopo;
                changed += 1;
            }
        }
        previous = Some((time, lanes));
    }
    changed
}

/// Re-flag every difficulty of a chart. Returns what changed, in the
/// file's own order.
pub fn apply_hopo_rule(chart: &mut ChartFile) -> Vec<(Difficulty, usize)> {
    let beats = Beats::of(chart);
    chart
        .charts
        .iter_mut()
        .map(|def: &mut ChartDef| (def.difficulty, reflag_hopos(&mut def.notes, &beats)))
        .collect()
}

/// Give the chart the strum-grace rule ([`STRUM_GRACE_MS`]). Returns
/// whether its rules changed. Every other rule the chart carries is
/// kept as it was.
pub fn apply_strum_rule(chart: &mut ChartFile) -> bool {
    let mut rules = chart.rules.unwrap_or_default();
    if rules.strum_grace_ms == STRUM_GRACE_MS {
        return false;
    }
    rules.strum_grace_ms = STRUM_GRACE_MS;
    chart.rules = Some(rules);
    true
}

/// Which ingredients a twin carries.
///
/// One flag per ingredient, and the directive it writes names them,
/// so a chart on disk says what was done to it and a later run can
/// tell a twin made under one recipe from a twin made under another.
/// The programme adds ingredients here rather than changing what
/// `hopo` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // one switch per ingredient is the point
pub struct Recipe {
    /// The early games' HOPO threshold: beats, not seconds (K1).
    pub hopo: bool,
    /// A strum under the wrong fret waits for it (K2).
    pub strum: bool,
}

impl Default for Recipe {
    /// Everything that has been through a blind test.
    fn default() -> Recipe {
        Recipe {
            hopo: true,
            strum: false,
        }
    }
}

impl Recipe {
    /// Every ingredient name the programme knows, in the order they
    /// were measured to matter — the order [`Recipe::names`] writes
    /// them in and [`Recipe::parse`] accepts.
    pub const ALL: [&'static str; 2] = ["hopo", "strum"];

    /// Nothing at all — the recipe that must never be written.
    #[must_use]
    pub const fn none() -> Recipe {
        Recipe {
            hopo: false,
            strum: false,
        }
    }

    /// Every ingredient switched on.
    #[must_use]
    pub const fn all() -> Recipe {
        Recipe {
            hopo: true,
            strum: true,
        }
    }

    /// The ingredients, in the order they were measured to matter.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.hopo {
            names.push("hopo");
        }
        if self.strum {
            names.push("strum");
        }
        names
    }

    /// What the chart's provenance says it carries.
    #[must_use]
    pub fn directive(self) -> String {
        format!("classic:{}", self.names().join("+"))
    }

    /// A recipe from a comma-separated list of ingredient names
    /// (`"hopo,strum"`), or `"all"`. Pure — tested.
    ///
    /// # Errors
    /// On a name the programme does not know, naming the ones it does.
    pub fn parse(text: &str) -> Result<Recipe, String> {
        let mut recipe = Recipe::none();
        for name in text.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match name {
                "all" => recipe = Recipe::all(),
                "hopo" => recipe.hopo = true,
                "strum" => recipe.strum = true,
                other => {
                    return Err(format!(
                        "no ingredient `{other}` — known: {}, or `all`",
                        Recipe::ALL.join(", ")
                    ));
                }
            }
        }
        Ok(recipe)
    }
}

/// What a recipe changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// `(difficulty, notes changed)` in the file's own order.
    pub notes: Vec<(Difficulty, usize)>,
    /// Whether the chart's judgment rules changed.
    pub rules: bool,
}

impl Changes {
    /// Whether nothing changed at all — the chart already plays by
    /// the recipe.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.rules && self.notes.iter().all(|(_, n)| *n == 0)
    }

    /// Notes changed across every difficulty.
    #[must_use]
    pub fn total_notes(&self) -> usize {
        self.notes.iter().map(|(_, n)| *n).sum()
    }
}

/// Apply a recipe. Each ingredient that is off does nothing.
pub fn apply(chart: &mut ChartFile, recipe: Recipe) -> Changes {
    let mut notes: Vec<(Difficulty, usize)> =
        chart.charts.iter().map(|c| (c.difficulty, 0)).collect();
    if recipe.hopo {
        for (slot, (difficulty, changed)) in notes.iter_mut().zip(apply_hopo_rule(chart)) {
            debug_assert_eq!(slot.0, difficulty);
            slot.1 += changed;
        }
    }
    let rules = recipe.strum && apply_strum_rule(chart);
    Changes { notes, rules }
}

/// What a twin-writing run came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Written: the folder, the title and what changed per difficulty.
    Written {
        /// Where the twin lives.
        folder: PathBuf,
        /// Its title, prefix included.
        title: String,
        /// `(difficulty id, notes changed)` in chart order.
        changed: Vec<(String, usize)>,
        /// Whether the chart's judgment rules changed.
        rules: bool,
        /// Whether the analysis sidecar came along.
        sidecar: bool,
    },
    /// A twin was already there; nothing written.
    AlreadyThere(PathBuf),
    /// The recipe changes nothing here; a twin identical to its
    /// source would be a second entry in the browser that plays the
    /// same chart.
    Refused(String),
}

/// Write the `[CL]` twin of `song_folder`.
///
/// ⚠️ **No audio is read and no chart is generated.** The twin is the
/// folder's ACTIVE chart with the recipe applied — same notes, same
/// times, same frets, same sustains — because the whole point of the
/// programme is that a blind test answers a question about ONE
/// variable. That also makes a twin of a `[GS]` twin meaningful and
/// cheap: it is the study chart, played by the classic rules.
///
/// # Errors
/// When the folder cannot be read, its active chart cannot be loaded
/// or is invalid, or the twin cannot be written.
pub fn write_twin(song_folder: &Path, recipe: Recipe) -> Result<Outcome, String> {
    let out =
        twin::folder_for(song_folder, KIND).ok_or("the song folder needs a name and a parent")?;
    if twin::is_finished(&out) {
        return Ok(Outcome::AlreadyThere(out));
    }
    if recipe.names().is_empty() {
        return Ok(Outcome::Refused(
            "no ingredients: a twin would be a copy".to_owned(),
        ));
    }
    let names = twin::names_in(song_folder)?;
    // A folder from the old layout (its chart named after the song)
    // has nothing to make a twin from. The redesign calls that
    // skipped rather than failed, and so does this.
    if !names.iter().any(|n| n == versions::BASE_CHART) {
        return Ok(Outcome::Refused(format!(
            "no `{}` — legacy layout",
            versions::BASE_CHART
        )));
    }
    let pointer = std::fs::read_to_string(song_folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active_path = song_folder.join(&active_name);
    let active = load_chart_file(&active_path)
        .map_err(|error| format!("cannot load {active_name}: {error}"))?;
    let problems = errors_of(&active);
    if !problems.is_empty() {
        return Err(format!("{active_name} is invalid: {}", problems.join("; ")));
    }

    let mut chart = active.clone();
    chart.song.title = twin::titled(&active.song.title, KIND);
    let changed = apply(&mut chart, recipe);
    if changed.is_empty() {
        return Ok(Outcome::Refused(format!(
            "{}: the chart already plays by these rules",
            active.song.title
        )));
    }
    chart.provenance = Some(Provenance {
        parent_hash: chart_hash(&active),
        designer: DESIGNER.to_owned(),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        directive: Some(recipe.directive()),
    });
    let problems = errors_of(&chart);
    if !problems.is_empty() {
        return Err(format!(
            "the classic chart is invalid: {}",
            problems.join("; ")
        ));
    }

    // The only write boundary: a NEW folder. Audio and sidecars
    // first, the chart LAST, so a library scan that happens mid-write
    // never sees a chart without its audio.
    std::fs::create_dir_all(&out)
        .map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    twin::copy_assets(song_folder, &out, &names)?;
    let chart_path = out.join(versions::BASE_CHART);
    save_chart_file(&chart_path, &chart)
        .map_err(|error| format!("cannot write the classic chart: {error}"))?;
    // After the chart, because it names the chart it describes.
    let sidecar = context::carry(&active_path, &active, &chart_path, &chart)?;
    Ok(Outcome::Written {
        folder: out,
        title: chart.song.title.clone(),
        changed: changed
            .notes
            .into_iter()
            .map(|(difficulty, n)| (difficulty.id().to_owned(), n))
            .collect(),
        rules: changed.rules,
        sidecar,
    })
}

fn errors_of(chart: &ChartFile) -> Vec<String> {
    chart
        .validate()
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SongMeta;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!("bb-cl-{tag}-{unique}"));
            std::fs::create_dir_all(&dir).expect("scratch");
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A song folder at 120 BPM whose notes sit an EIGHTH apart —
    /// 0.25 s, which the old rule in seconds calls a hammer-on and
    /// the classic rule in beats (0.5 of a beat, over 0.354) does
    /// not. So the twin has something to change.
    fn a_song_folder(dir: &Path, hopo: bool) -> ChartFile {
        std::fs::create_dir_all(dir).expect("dir");
        std::fs::write(dir.join("t.wav"), b"audio").expect("audio");
        std::fs::write(dir.join("t.lrc"), b"lyrics").expect("lyrics");
        let chart = ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Maria".into(),
                artist: "Blondie".into(),
                audio: "t.wav".into(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(30.0),
                genre: None,
            },
            charts: Difficulty::ALL
                .iter()
                .map(|d| ChartDef {
                    difficulty: *d,
                    lanes: 5,
                    notes: (0..8)
                        .map(|i| ChartNote {
                            time: 1.0 + f64::from(i) * 0.25,
                            lane: (i % 5) as u8,
                            len: 0.0,
                            hopo: hopo && i > 0,
                        })
                        .collect(),
                    phrases: vec![],
                })
                .collect(),
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        };
        save_chart_file(&dir.join(versions::BASE_CHART), &chart).expect("chart");
        chart
    }

    /// The whole contract of a twin in one run: it appears beside the
    /// original with the prefix and the changed flags, it carries the
    /// song's assets and none of the charts, its provenance names
    /// what was done, and — the part that matters most — **the
    /// original is not written to at all**.
    #[test]
    fn a_twin_is_the_active_chart_by_the_classic_rules_and_the_original_is_untouched() {
        let scratch = Scratch::new("write");
        let song = scratch.0.join("blondie---maria-m4a");
        a_song_folder(&song, true);
        let before = std::fs::read(song.join(versions::BASE_CHART)).expect("read");

        let outcome = write_twin(&song, Recipe::default()).expect("the twin must be written");
        let Outcome::Written {
            folder,
            title,
            changed,
            ..
        } = &outcome
        else {
            panic!("not written: {outcome:?}");
        };
        assert_eq!(folder, &scratch.0.join("classic-blondie---maria-m4a"));
        assert_eq!(title, "[CL] Maria");
        assert!(
            changed.iter().all(|(_, n)| *n == 7),
            "every eighth after the first should have lost its flag: {changed:?}"
        );

        // The assets travel; the charts do not.
        assert!(folder.join("t.wav").is_file(), "the audio did not travel");
        assert!(folder.join("t.lrc").is_file(), "the lyrics did not travel");
        assert!(!folder.join(versions::POINTER_FILE).exists());

        let twin_chart = load_chart_file(&folder.join(versions::BASE_CHART)).expect("twin chart");
        assert!(
            twin_chart
                .charts
                .iter()
                .all(|c| !c.notes.iter().any(|n| n.hopo)),
            "a straight eighth is never a hammer-on under the classic rule"
        );
        let provenance = twin_chart.provenance.as_ref().expect("provenance");
        assert_eq!(provenance.designer, DESIGNER);
        assert_eq!(provenance.directive.as_deref(), Some("classic:hopo"));

        // ⚠️ The original: byte for byte what it was.
        assert_eq!(
            std::fs::read(song.join(versions::BASE_CHART)).expect("read"),
            before,
            "the original was written to"
        );
    }

    /// ⚠️ A chart that already plays by these rules would give a
    /// second browser entry playing the same notes. Refused before
    /// anything is written — the folder must not even appear, or the
    /// next run would call the empty folder a finished twin.
    #[test]
    fn a_chart_that_would_not_change_gets_no_twin() {
        let scratch = Scratch::new("nochange");
        let song = scratch.0.join("blondie---maria-m4a");
        a_song_folder(&song, false);
        let outcome = write_twin(&song, Recipe::default()).expect("no error");
        assert!(
            matches!(&outcome, Outcome::Refused(reason) if reason.contains("already plays")),
            "{outcome:?}"
        );
        assert!(
            !scratch.0.join("classic-blondie---maria-m4a").exists(),
            "a refused run left a folder behind"
        );
    }

    /// A finished twin ends the run before anything is read, so a
    /// second pass over a library is cheap and changes nothing.
    #[test]
    fn a_folder_that_already_has_its_twin_is_left_alone() {
        let scratch = Scratch::new("already");
        let song = scratch.0.join("blondie---maria-m4a");
        a_song_folder(&song, true);
        write_twin(&song, Recipe::default()).expect("first");
        let twin_path = scratch.0.join("classic-blondie---maria-m4a");
        let written = std::fs::read(twin_path.join(versions::BASE_CHART)).expect("read");
        let outcome = write_twin(&song, Recipe::default()).expect("second");
        assert_eq!(outcome, Outcome::AlreadyThere(twin_path.clone()));
        assert_eq!(
            std::fs::read(twin_path.join(versions::BASE_CHART)).expect("read"),
            written,
            "the second run rewrote the twin"
        );
    }

    /// A twin of a twin is a twin OF THE STUDY: the prefix stacks,
    /// which is how the browser files it under the chart it was made
    /// from rather than under the mix.
    #[test]
    fn a_twin_of_a_study_keeps_the_study_in_its_name() {
        let scratch = Scratch::new("chain");
        let study = scratch.0.join("guitar-study-blondie---maria-m4a");
        let mut chart = a_song_folder(&study, true);
        chart.song.title = "[GS] Maria".into();
        save_chart_file(&study.join(versions::BASE_CHART), &chart).expect("chart");
        let outcome = write_twin(&study, Recipe::default()).expect("written");
        let Outcome::Written { folder, title, .. } = &outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(title, "[CL] [GS] Maria");
        assert_eq!(
            folder,
            &scratch.0.join("classic-guitar-study-blondie---maria-m4a")
        );
        assert_eq!(crate::twin::base_title(title), Some("[GS] Maria"));
    }

    /// ⚠️ An ingredient that is switched OFF must do nothing, and
    /// nothing else in the suite says so: the twin path refuses an
    /// empty recipe before it ever calls this, so removing the guard
    /// left every test green while `apply` cooked regardless of what
    /// it was handed. That is the whole promise of the programme —
    /// one ingredient at a time — and it is checked here.
    #[test]
    fn an_ingredient_that_is_off_changes_nothing() {
        let scratch = Scratch::new("off");
        let song = scratch.0.join("blondie---maria-m4a");
        let with_flags = a_song_folder(&song, true);
        let mut chart = with_flags.clone();
        let changed = apply(&mut chart, Recipe::none());
        assert!(
            changed.is_empty(),
            "an empty recipe reported changes: {changed:?}"
        );
        assert_eq!(
            crate::chart_hash(&chart),
            crate::chart_hash(&with_flags),
            "an empty recipe changed the chart"
        );
        // And the ingredient switched ON does change it, or the test
        // above would pass on a chart that had nothing to change.
        let mut cooked = with_flags.clone();
        let changed = apply(&mut cooked, Recipe::default());
        assert!(!changed.is_empty(), "{changed:?}");
    }

    /// The directive names the ingredients, so a chart on disk says
    /// what was done to it — and an empty recipe never writes at all.
    #[test]
    fn the_recipe_names_what_it_carries() {
        assert_eq!(Recipe::default().directive(), "classic:hopo");
        assert_eq!(Recipe::default().names(), vec!["hopo"]);
        assert!(Recipe::none().names().is_empty());
        let scratch = Scratch::new("empty");
        let song = scratch.0.join("blondie---maria-m4a");
        a_song_folder(&song, true);
        let outcome = write_twin(&song, Recipe::none()).expect("no error");
        assert!(
            matches!(&outcome, Outcome::Refused(reason) if reason.contains("no ingredients")),
            "{outcome:?}"
        );
    }

    /// ⚠️ K2 is the one ingredient that touches no note: it is a rule
    /// the chart carries. So its twin is the same notes, bit for bit,
    /// with the rule added — and the engine then plays it by that rule.
    #[test]
    fn the_strum_ingredient_adds_the_rule_and_touches_no_note() {
        let scratch = Scratch::new("strum");
        let song = scratch.0.join("blondie---maria-m4a");
        let source = a_song_folder(&song, false);
        let recipe = Recipe {
            strum: true,
            ..Recipe::none()
        };
        let outcome = write_twin(&song, recipe).expect("written");
        let Outcome::Written {
            folder,
            changed,
            rules,
            ..
        } = &outcome
        else {
            panic!("{outcome:?}");
        };
        assert!(*rules, "the rule was not reported");
        assert!(changed.iter().all(|(_, n)| *n == 0), "{changed:?}");
        let twin = load_chart_file(&folder.join(versions::BASE_CHART)).expect("twin");
        assert_eq!(twin.rules.map(|r| r.strum_grace_ms), Some(STRUM_GRACE_MS));
        for (new, old) in twin.charts.iter().zip(&source.charts) {
            assert_eq!(
                new.notes, old.notes,
                "a note changed under a rule-only recipe"
            );
        }
        assert_eq!(
            twin.provenance
                .as_ref()
                .and_then(|p| p.directive.as_deref()),
            Some("classic:strum")
        );
        let track = twin.to_track(Difficulty::Hard).expect("track");
        assert!((track.strum_grace_s() - 0.06).abs() < 1e-12);
    }

    /// A chart that already carries the rule gets no twin for it — and
    /// a rule the chart carried for another reason survives the
    /// ingredient rather than being replaced by a default.
    #[test]
    fn the_strum_rule_is_idempotent() {
        let mut chart = ChartFile::from_json(
            r#"{"format_version":1,"song":{"title":"T","artist":"A","audio":"a.m4a",
                "bpm":120.0,"offset_s":0.0},
                "charts":[{"difficulty":"hard","lanes":5,"notes":[{"time":1.0,"lane":0}]}]}"#,
        )
        .expect("parses");
        assert!(apply_strum_rule(&mut chart));
        assert!(
            !apply_strum_rule(&mut chart),
            "the second pass changed it again"
        );
        let strum_only = Recipe {
            strum: true,
            ..Recipe::none()
        };
        assert!(apply(&mut chart, strum_only).is_empty());
    }

    /// The CLI's `--with` list, and the names the directive writes.
    #[test]
    fn a_recipe_is_read_from_its_names() {
        assert_eq!(Recipe::parse("hopo").expect("parses"), Recipe::default());
        assert_eq!(
            Recipe::parse(" strum , hopo ").expect("parses"),
            Recipe::all(),
            "order or spaces mattered"
        );
        assert_eq!(Recipe::parse("all").expect("parses"), Recipe::all());
        assert_eq!(Recipe::parse("").expect("parses"), Recipe::none());
        let error = Recipe::parse("hopo,nonsense").expect_err("an unknown name");
        assert!(
            error.contains("nonsense") && error.contains("strum"),
            "{error}"
        );
        // Every name the programme lists round-trips through the
        // recipe, and the directive names them in that order.
        for name in Recipe::ALL {
            assert_eq!(Recipe::parse(name).expect("parses").names(), vec![name]);
        }
        assert_eq!(Recipe::all().names(), Recipe::ALL.to_vec());
        assert_eq!(Recipe::all().directive(), "classic:hopo+strum");
    }

    /// A chart of single notes at `times`, all on alternating lanes
    /// so the fret rule never gets in the way of the timing rule.
    fn singles(times: &[f64]) -> Vec<ChartNote> {
        times
            .iter()
            .enumerate()
            .map(|(i, t)| ChartNote {
                time: *t,
                lane: (i % 5) as u8,
                len: 0.0,
                hopo: false,
            })
            .collect()
    }

    fn flags(notes: &[ChartNote]) -> Vec<bool> {
        notes.iter().map(|n| n.hopo).collect()
    }

    /// ⚠️ The ingredient itself: at 120 BPM a straight eighth is
    /// 0.25 s and a beat is 0.5 s, so the gap is exactly half a beat
    /// — over the threshold, strummed. An eighth-note triplet is
    /// a third of a beat, under it, hammered. The old rule asked in
    /// seconds and called both of them HOPOs.
    #[test]
    fn straight_eighths_are_strummed_and_triplets_are_hammered() {
        let beat = 0.5; // 120 BPM
        let beats = Beats::constant(beat);

        let mut eighths = singles(&[0.0, 0.25, 0.5, 0.75]);
        reflag_hopos(&mut eighths, &beats);
        assert_eq!(
            flags(&eighths),
            vec![false; 4],
            "a straight eighth hammered"
        );

        // ⚠️ All THREE of the notes after the first hammer, the
        // fourth included: it sits a third of a beat after the one
        // before it like the others do, and a beat-relative rule
        // reads gaps, not bar positions. I wrote `false` here first
        // and the test corrected the arithmetic.
        let mut triplets = singles(&[0.0, beat / 3.0, 2.0 * beat / 3.0, beat]);
        reflag_hopos(&mut triplets, &beats);
        assert_eq!(
            flags(&triplets),
            vec![false, true, true, true],
            "the triplets did not hammer"
        );
        // A note a whole beat later is a strum again.
        let mut after = singles(&[0.0, beat / 3.0, 2.0 * beat]);
        reflag_hopos(&mut after, &beats);
        assert_eq!(flags(&after), vec![false, true, false]);

        let mut sixteenths = singles(&[0.0, 0.125, 0.25, 0.375]);
        reflag_hopos(&mut sixteenths, &beats);
        assert_eq!(flags(&sixteenths), vec![false, true, true, true]);
    }

    /// ⚠️ Exclusive, not inclusive. A gap of exactly the threshold
    /// is a strum — the engine's own comparison is `<`, and a rule
    /// that rounds the other way turns a whole class of notes over.
    #[test]
    fn a_gap_exactly_at_the_threshold_is_strummed() {
        let beats = Beats::constant(1.0);
        let mut at = singles(&[0.0, HOPO_BEATS]);
        reflag_hopos(&mut at, &beats);
        assert_eq!(flags(&at), vec![false, false], "the boundary hammered");

        let mut under = singles(&[0.0, HOPO_BEATS - 1e-9]);
        reflag_hopos(&mut under, &beats);
        assert_eq!(flags(&under), vec![false, true]);
    }

    /// The rule is about the LOCAL beat, so the same gap answers
    /// differently in a fast bar and a slow one. A chart's constant
    /// `bpm` is not its grid.
    #[test]
    fn the_same_gap_answers_differently_at_different_tempi() {
        let gap = 0.2;
        // At 60 BPM a beat is a second: 0.2 s is a fifth of it.
        let mut slow = singles(&[0.0, gap]);
        reflag_hopos(&mut slow, &Beats::constant(1.0));
        assert_eq!(flags(&slow), vec![false, true]);
        // At 200 BPM a beat is 0.3 s: the same gap is two thirds.
        let mut fast = singles(&[0.0, gap]);
        reflag_hopos(&mut fast, &Beats::constant(0.3));
        assert_eq!(flags(&fast), vec![false, false]);
    }

    /// A tracked grid that slows down is followed, note by note.
    #[test]
    fn a_tracked_grid_is_followed_rather_than_the_constant_bpm() {
        // Beats a second apart, then half a second apart.
        let grid = BeatGrid {
            beats: vec![0.0, 1.0, 2.0, 2.5, 3.0, 3.5],
            downbeats: Vec::new(),
        };
        let beats = Beats {
            grid: Some(grid),
            constant_s: 1.0,
        };
        // A 0.2 s gap early (a fifth of a beat) hammers; the same gap
        // late (two fifths of a half-second beat) does not.
        let mut notes = singles(&[0.0, 0.2]);
        reflag_hopos(&mut notes, &beats);
        assert_eq!(flags(&notes), vec![false, true], "the early gap strummed");

        let mut late = singles(&[2.5, 2.7]);
        reflag_hopos(&mut late, &beats);
        assert_eq!(flags(&late), vec![false, false], "the late gap hammered");
    }

    /// A fret the hand already has down is a strum, whether it came
    /// from the note before or from the chord before.
    #[test]
    fn a_repeated_fret_is_strummed_after_a_note_and_after_a_chord() {
        let beats = Beats::constant(1.0);
        let close = 0.1;

        let mut repeat = vec![
            ChartNote {
                time: 0.0,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: close,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
        ];
        reflag_hopos(&mut repeat, &beats);
        assert_eq!(
            flags(&repeat),
            vec![false, false],
            "a repeated fret hammered"
        );

        // A chord, then one of its own frets, then a fret it did not
        // contain. ⚠️ The middle one is the case the early engines
        // call out by name: it was already held, so it is a strum.
        let mut after_chord = vec![
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.0,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: close,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 2.0 * close,
                lane: 3,
                len: 0.0,
                hopo: false,
            },
        ];
        reflag_hopos(&mut after_chord, &beats);
        assert_eq!(
            flags(&after_chord),
            vec![false, false, false, true],
            "a fret out of the chord hammered, or the new fret did not"
        );
    }

    /// Chords are strummed in all of these games, however close they
    /// fall — and a chord that arrives already flagged is cleared.
    #[test]
    fn a_chord_is_never_a_hammer_on() {
        let beats = Beats::constant(1.0);
        let mut notes = vec![
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.1,
                lane: 2,
                len: 0.0,
                hopo: true,
            },
            ChartNote {
                time: 0.1,
                lane: 3,
                len: 0.0,
                hopo: true,
            },
        ];
        let changed = reflag_hopos(&mut notes, &beats);
        assert_eq!(flags(&notes), vec![false, false, false]);
        assert_eq!(changed, 2, "the chord's flags were not cleared");
    }

    /// Simultaneous within the engine's own window is one chord —
    /// not two events a tenth of a millisecond apart, which would be
    /// a hammer-on the hand cannot play.
    #[test]
    fn notes_a_hair_apart_are_one_chord_the_way_the_engine_reads_them() {
        let beats = Beats::constant(1.0);
        let apart = crate::convert::CHORD_EPSILON_S * 0.5;
        let mut notes = vec![
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: apart,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
        ];
        reflag_hopos(&mut notes, &beats);
        assert_eq!(flags(&notes), vec![false, false], "a chord split in two");
    }

    /// Nothing says how long a beat is → nothing is a hammer-on.
    /// Silence is the safe answer; guessing a tempo is not.
    #[test]
    fn without_a_beat_nothing_hammers() {
        let mut notes = singles(&[0.0, 0.01, 0.02]);
        reflag_hopos(&mut notes, &Beats::constant(0.0));
        assert_eq!(flags(&notes), vec![false; 3]);
    }

    /// ⚠️⚠️ The pin the whole method rests on: an ingredient may
    /// change the `hopo` flags and **nothing else**. The blind test
    /// plays the new version against its parent, and a person can
    /// only answer a question about one variable — if a note, a
    /// fret, a time, a sustain or a phrase moved as well, the
    /// verdict would be about a different question than the one
    /// asked.
    #[test]
    fn an_ingredient_changes_the_flags_and_nothing_else() {
        let text = r#"{"format_version":1,
            "song":{"title":"Maria","artist":"Blondie","audio":"maria.m4a",
                    "bpm":132.0,"duration_s":200.0,"offset_s":0.0,
                    "preview_start_s":31.5},
            "grid":{"beats":[0.0,0.45,0.9,1.35,1.8,2.25,2.7]},
            "charts":[
              {"difficulty":"hard","lanes":5,
               "notes":[{"time":0.0,"lane":0,"len":1.25},
                        {"time":0.22,"lane":2},
                        {"time":0.45,"lane":2,"hopo":true},
                        {"time":0.52,"lane":3,"hopo":true},
                        {"time":0.9,"lane":1},
                        {"time":0.9,"lane":3},
                        {"time":1.8,"lane":4,"len":0.5}],
               "phrases":[{"start":0.0,"end":1.8}]},
              {"difficulty":"expert","lanes":5,
               "notes":[{"time":0.1,"lane":1},{"time":0.2,"lane":4,"hopo":true}],
               "phrases":[]}
            ]}"#;
        let before = ChartFile::from_json(text).expect("the fixture parses");
        let mut after = before.clone();
        let changed = apply_hopo_rule(&mut after);
        assert!(
            changed.iter().any(|(_, n)| *n > 0),
            "the fixture exercises nothing: {changed:?}"
        );

        // Everything outside the flags, read back field by field.
        assert_eq!(after.song, before.song, "the song metadata moved");
        assert_eq!(after.grid, before.grid, "the grid moved");
        assert_eq!(after.audio_trim, before.audio_trim);
        assert_eq!(after.format_version, before.format_version);
        assert_eq!(after.charts.len(), before.charts.len());
        for (new, old) in after.charts.iter().zip(&before.charts) {
            assert_eq!(new.difficulty, old.difficulty);
            assert_eq!(new.lanes, old.lanes);
            assert_eq!(new.phrases, old.phrases, "a phrase moved");
            assert_eq!(new.notes.len(), old.notes.len(), "a note appeared or left");
            for (a, b) in new.notes.iter().zip(&old.notes) {
                assert_eq!(a.time, b.time, "a note moved in time");
                assert_eq!(a.lane, b.lane, "a note changed fret");
                assert_eq!(a.len, b.len, "a sustain changed length");
            }
        }
        // And the counts it reports are the flags that really moved.
        for (difficulty, count) in changed {
            let new = after.charts.iter().find(|c| c.difficulty == difficulty);
            let old = before.charts.iter().find(|c| c.difficulty == difficulty);
            let really = new
                .zip(old)
                .map(|(n, o)| {
                    n.notes
                        .iter()
                        .zip(&o.notes)
                        .filter(|(a, b)| a.hopo != b.hopo)
                        .count()
                })
                .unwrap_or(0);
            assert_eq!(count, really, "{difficulty:?} miscounted its changes");
        }
    }

    /// The file's note ORDER survives, because the hash sees it.
    #[test]
    fn the_notes_keep_the_order_the_file_had() {
        let beats = Beats::constant(1.0);
        // Deliberately out of time order, as a hand-edited file may be.
        let mut notes = vec![
            ChartNote {
                time: 0.5,
                lane: 1,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.1,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
        ];
        let before: Vec<(f64, u8)> = notes.iter().map(|n| (n.time, n.lane)).collect();
        reflag_hopos(&mut notes, &beats);
        let after: Vec<(f64, u8)> = notes.iter().map(|n| (n.time, n.lane)).collect();
        assert_eq!(before, after, "the notes were reordered");
        // And the rule still read them in time order: 0.1 follows
        // 0.0 closely on another fret.
        assert_eq!(flags(&notes), vec![false, false, true]);
    }
}
