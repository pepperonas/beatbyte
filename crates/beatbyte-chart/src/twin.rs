//! What every twin of a song folder has in common.
//!
//! A twin is a chart in a folder of its own beside the original: the
//! same audio and the same sidecars, one `chart.json` whose title
//! carries a prefix, so both appear in the browser and can be played
//! against each other. The original — its versions, its pointer, its
//! telemetry — is never written to.
//!
//! There are two kinds now ([`Kind`]), and the things that must know
//! about twins must know about BOTH: a redesign has to refuse them
//! all (it would read the mix and bury the twin's chart under a new
//! active version), the catalogue must skip them all (same recording,
//! same question, somebody's rate limit), and the browser has to file
//! them all under the song they belong to. Every one of those rules
//! lived on the study's own prefix; a second kind would have slipped
//! past all three, so the rules live here and the kinds are a list.

use std::path::{Path, PathBuf};

use crate::versions;

/// A kind of twin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `[GS]` — charted from a separated instrument stem.
    Study,
    /// `[CL]` — the classic ingredients applied to an existing chart.
    Classic,
}

impl Kind {
    /// Every kind, in the order they were introduced.
    pub const ALL: [Kind; 2] = [Kind::Study, Kind::Classic];

    /// The title prefix the browser shows this kind under.
    #[must_use]
    pub const fn title_prefix(self) -> &'static str {
        match self {
            Kind::Study => "[GS] ",
            Kind::Classic => "[CL] ",
        }
    }

    /// The folder-name prefix. ⚠️ The FOLDER decides what a twin is,
    /// never the title: a player may rename a song, and one of them
    /// here had been.
    #[must_use]
    pub const fn folder_prefix(self) -> &'static str {
        match self {
            Kind::Study => "guitar-study-",
            Kind::Classic => "classic-",
        }
    }
}

/// The title prefix an older BeatByte wrote for a study.
const LEGACY_STUDY_PREFIX: &str = "[Guitar Study] ";

/// Which kind of twin a folder name names, if any.
#[must_use]
pub fn kind_of_folder(name: &str) -> Option<Kind> {
    Kind::ALL
        .into_iter()
        .find(|kind| name.starts_with(kind.folder_prefix()))
}

/// Whether a folder name is any twin's.
#[must_use]
pub fn is_twin_folder(name: &str) -> bool {
    kind_of_folder(name).is_some()
}

/// The folder a twin of `song_folder` lives in (or would).
#[must_use]
pub fn folder_for(song_folder: &Path, kind: Kind) -> Option<PathBuf> {
    let name = song_folder.file_name()?.to_string_lossy().into_owned();
    Some(
        song_folder
            .parent()?
            .join(format!("{}{name}", kind.folder_prefix())),
    )
}

/// The title one prefix behind a twin's, or `None` for a title that
/// is not a twin's.
///
/// ⚠️ **One prefix, not all of them.** A classic twin of a study is
/// `[CL] [GS] Maria`, and what it is a twin OF is `[GS] Maria` —
/// stripping both would file it under the mix, which is not the
/// chart it was made from. Pure — tested.
#[must_use]
pub fn base_title(title: &str) -> Option<&str> {
    Kind::ALL
        .into_iter()
        .find_map(|kind| title.strip_prefix(kind.title_prefix()))
        .or_else(|| title.strip_prefix(LEGACY_STUDY_PREFIX))
}

/// A twin's title with every prefix written the way this build
/// writes it, so a chart saved by an older version still reads as
/// the twin it is. The chart file itself is left untouched.
///
/// ⚠️ It walks the WHOLE chain, not just the front. A classic twin
/// of a study saved under the long prefix reads `[CL] [Guitar Study]
/// Maria`, and the study it belongs to is listed as `[GS] Maria` —
/// so the browser looked for an original that, spelled that way,
/// does not exist, and the twin lost its place in the list.
#[must_use]
pub fn display_title(title: &str) -> String {
    if let Some(rest) = title.strip_prefix(LEGACY_STUDY_PREFIX) {
        return format!("{}{}", Kind::Study.title_prefix(), display_title(rest));
    }
    match Kind::ALL
        .into_iter()
        .find(|kind| title.starts_with(kind.title_prefix()))
    {
        Some(kind) => format!(
            "{}{}",
            kind.title_prefix(),
            display_title(&title[kind.title_prefix().len()..])
        ),
        None => title.to_owned(),
    }
}

/// `title` with this kind's prefix, added once and never twice.
/// Pure — tested.
#[must_use]
pub fn titled(title: &str, kind: Kind) -> String {
    // Through `display_title` first: a twin written on top of a
    // title in the long legacy spelling would otherwise carry it for
    // ever, and the browser cannot pair what it cannot spell.
    let base = display_title(title);
    let base = base.strip_prefix(kind.title_prefix()).unwrap_or(&base);
    format!("{}{base}", kind.title_prefix())
}

/// The song document's file name.
///
/// ⚠️ Spelled out rather than imported: `beatbyte-library` owns the
/// constant and depends on THIS crate, so taking it from there would
/// be a cycle. A test over in that crate compares the two, which is
/// the one place that can see both.
pub const DOC_FILE: &str = "song.json";

/// Whether a file in a song folder belongs to the CHART or to the
/// song's IDENTITY rather than to the recording — the things a twin
/// must NOT inherit; everything else (audio, lyrics, loudness) comes
/// along.
///
/// ⚠️ **The song document stays behind.** It carries the `song_id`
/// that scores, records and recorded sessions are keyed by, and a
/// copy hands the twin its source's identity: two different charts
/// filed as one song. The 85 study twins in this library escaped it
/// only because documents did not exist yet when they were written —
/// the first classic twin produced the first duplicate id on disk.
/// Without a document a twin has none until it is played, and the
/// one written then is its own.
///
/// ⚠️ The analysis sidecar belongs to a chart too. It names the chart
/// it describes by content hash, so a copied one describes the
/// ORIGINAL's chart and is discarded on every read — and the sidecar
/// of a version the twin does not even have describes a file that is
/// not there at all. A twin writes its own or has none.
/// Pure — tested.
#[must_use]
pub fn stays_behind(name: &str) -> bool {
    if let Some(stem) = name.strip_suffix(".context.json") {
        return stays_behind(&format!("{stem}.json"));
    }
    name == DOC_FILE
        || name == versions::BASE_CHART
        || name == versions::POINTER_FILE
        || versions::is_version_file(name)
}

/// Whether a twin folder holds a FINISHED twin.
///
/// ⚠️ Existing is not enough. The chart is written last, on purpose,
/// so a library scan never sees a chart without its audio — which
/// means a run that failed partway leaves a folder that exists and
/// plays nothing. Answering "already there" to that one is how a
/// half-written twin became permanent: every later run saw the
/// folder, returned early, and never got as far as the failure.
#[must_use]
pub fn is_finished(folder: &Path) -> bool {
    folder.join(versions::BASE_CHART).is_file()
}

/// Every file name in a song folder.
///
/// # Errors
/// When the folder cannot be listed.
pub fn names_in(folder: &Path) -> Result<Vec<String>, String> {
    Ok(std::fs::read_dir(folder)
        .map_err(|error| format!("cannot read {}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

/// Copy a song folder's assets — everything but the charts — into
/// the twin's folder.
///
/// ⚠️ **Files only.** `fs::copy` fails on a directory, and a song
/// folder grows them: separated stems live in `<song>.stems`. The
/// failure landed AFTER the twin folder had been created, so the
/// half-written folder answered "already there" for ever and the
/// song never got a twin. Stems are derived from the audio beside
/// them and a twin can make its own.
///
/// Its own function so a test can call the real thing: the first
/// version of that test copied this loop into itself, which meant a
/// mutation of the loop changed nothing and the pin was blind.
///
/// # Errors
/// When a file cannot be copied.
pub fn copy_assets(from: &Path, to: &Path, names: &[String]) -> Result<(), String> {
    for name in names.iter().filter(|n| !stays_behind(n)) {
        let source = from.join(name);
        if !source.is_file() {
            continue;
        }
        std::fs::copy(&source, to.join(name))
            .map_err(|error| format!("cannot copy {name}: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!("bb-twin-{tag}-{unique}"));
            std::fs::create_dir_all(&dir).expect("scratch");
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// ⚠️ Two halves of one defect, and the second is what made the
    /// first permanent. A song folder grows subdirectories —
    /// separated stems live in `<song>.stems` — and `fs::copy` fails
    /// on a directory. That failure lands AFTER the twin folder has
    /// been created, so the folder exists, holds no chart, and every
    /// later run saw it, said "already there" and returned before
    /// reaching the failure again. The song never got a twin and
    /// nothing ever said why.
    #[test]
    fn a_folder_without_a_chart_is_not_a_finished_twin() {
        let scratch = Scratch::new("finished");
        let twin = scratch.0.join("guitar-study-song");
        assert!(!is_finished(&twin), "a missing folder counted");
        std::fs::create_dir_all(&twin).expect("dir");
        std::fs::write(twin.join("song.m4a"), b"audio").expect("audio");
        assert!(
            !is_finished(&twin),
            "a half-written twin counted as finished"
        );
        std::fs::write(twin.join(versions::BASE_CHART), b"{}").expect("chart");
        assert!(is_finished(&twin), "a finished twin did not count");
    }

    /// The copy walks FILES. A directory in the song folder is
    /// skipped rather than failing the run.
    #[test]
    fn a_subfolder_in_the_song_folder_is_skipped_rather_than_fatal() {
        let scratch = Scratch::new("copy");
        let from = scratch.0.join("song");
        let to = scratch.0.join("guitar-study-song");
        std::fs::create_dir_all(from.join("Song.stems")).expect("stems dir");
        std::fs::write(from.join("Song.stems").join("guitar.wav"), b"x").expect("stem");
        std::fs::write(from.join("Song.m4a"), b"audio").expect("audio");
        std::fs::write(from.join("Song.lrc"), b"lyrics").expect("lrc");
        std::fs::write(from.join(DOC_FILE), b"{}").expect("document");
        std::fs::write(from.join(versions::BASE_CHART), b"{}").expect("chart");
        std::fs::create_dir_all(&to).expect("out");

        let names: Vec<String> = std::fs::read_dir(&from)
            .expect("list")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // ⚠️ The REAL function. The first version of this test copied
        // the loop into itself, so mutating the loop changed nothing
        // and the pin was blind — the mutation probe said so.
        copy_assets(&from, &to, &names).expect("the copy must not fail");
        assert!(to.join("Song.m4a").is_file(), "the audio did not travel");
        assert!(to.join("Song.lrc").is_file(), "the lyrics did not travel");
        assert!(
            !to.join("Song.stems").exists(),
            "the stems folder was copied after all"
        );
        assert!(
            !to.join(versions::BASE_CHART).exists(),
            "the chart is written separately, last"
        );
        assert!(
            !to.join(DOC_FILE).exists(),
            "the twin inherited its source's song identity"
        );
    }

    /// A copy that cannot happen is an error, not a shrug: the twin
    /// would otherwise be written without its audio.
    #[test]
    fn a_copy_that_fails_fails_the_run() {
        let scratch = Scratch::new("copyfail");
        let from = scratch.0.join("song");
        std::fs::create_dir_all(&from).expect("dir");
        std::fs::write(from.join("Song.m4a"), b"audio").expect("audio");
        let names = vec!["Song.m4a".to_owned()];
        // The destination does not exist.
        let outcome = copy_assets(&from, &scratch.0.join("nowhere"), &names);
        assert!(
            matches!(&outcome, Err(reason) if reason.contains("cannot copy")),
            "a failed copy was swallowed: {outcome:?}"
        );
    }

    /// ⚠️ The rule that a twin is a twin by its FOLDER has to hold
    /// for every kind there is, because the three things that use it
    /// — the redesign's refusal, the catalogue's skip and the
    /// browser's filing — were each written against one prefix and
    /// would have let a second kind through all three.
    #[test]
    fn every_kind_is_recognised_by_its_folder() {
        for kind in Kind::ALL {
            let name = format!("{}blondie---maria-m4a", kind.folder_prefix());
            assert_eq!(kind_of_folder(&name), Some(kind), "{name}");
            assert!(is_twin_folder(&name));
        }
        assert_eq!(kind_of_folder("blondie---maria-m4a"), None);
        assert!(!is_twin_folder("blondie---maria-m4a"));
    }

    /// ⚠️ One prefix comes off, not all of them: a classic twin of a
    /// study is a twin OF THE STUDY, and stripping both would file it
    /// under the mix — a chart it was never made from.
    #[test]
    fn a_twin_of_a_twin_names_the_chart_it_was_made_from() {
        assert_eq!(base_title("[CL] [GS] Maria"), Some("[GS] Maria"));
        assert_eq!(base_title("[GS] Maria"), Some("Maria"));
        assert_eq!(base_title("[CL] Maria"), Some("Maria"));
        assert_eq!(base_title("Maria"), None);
        // The long prefix an older build wrote still reads.
        assert_eq!(base_title("[Guitar Study] Maria"), Some("Maria"));
        assert_eq!(display_title("[Guitar Study] Maria"), "[GS] Maria");
        assert_eq!(display_title("[CL] Maria"), "[CL] Maria");
        // ⚠️ The whole chain, not just its front: this is the shape
        // a classic twin of an older study really has on disk.
        assert_eq!(
            display_title("[CL] [Guitar Study] Maria"),
            "[CL] [GS] Maria"
        );
        assert_eq!(
            base_title(&display_title("[CL] [Guitar Study] Maria")),
            Some("[GS] Maria"),
            "the twin must name a title the browser can actually find"
        );
        // A song that merely mentions it keeps its name.
        assert_eq!(
            display_title("Songs About [Guitar Study] Nights"),
            "Songs About [Guitar Study] Nights"
        );
    }

    /// A prefix is added once. Running the writer twice must not
    /// produce `[CL] [CL] Maria`.
    #[test]
    fn a_prefix_is_added_once() {
        assert_eq!(titled("Maria", Kind::Classic), "[CL] Maria");
        assert_eq!(titled("[CL] Maria", Kind::Classic), "[CL] Maria");
        // But a DIFFERENT kind's prefix is kept: that is the chain.
        assert_eq!(titled("[GS] Maria", Kind::Classic), "[CL] [GS] Maria");
        // A study saved under the long prefix is spelled the new way.
        assert_eq!(
            titled("[Guitar Study] Maria", Kind::Classic),
            "[CL] [GS] Maria"
        );
    }

    /// ⚠️ The document is the sharpest of these. It carries the
    /// `song_id` that every score, record and recorded session is
    /// keyed by, so a copied one files two different charts as one
    /// song — measured on disk the moment the first classic twin was
    /// written, and invisible until then only because the 85 study
    /// twins predate documents existing at all.
    #[test]
    fn a_twin_takes_the_recording_and_neither_the_charts_nor_the_identity() {
        assert!(stays_behind("chart.json"));
        assert!(stays_behind("chart-active.json"));
        assert!(stays_behind("chart.v2.json"));
        // The analysis sidecar of a chart is the chart's, not the song's.
        assert!(stays_behind("chart.context.json"));
        assert!(stays_behind("chart.v2.context.json"));
        assert!(stays_behind(DOC_FILE));
        assert!(!stays_behind("song.m4a"));
        // …but a sidecar of something that is not a chart is the song's.
        assert!(!stays_behind("Song.words.context.json"));
        assert!(!stays_behind("words.json"));
    }
}
