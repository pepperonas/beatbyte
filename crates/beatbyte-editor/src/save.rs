//! Saving an edit: a new chart revision, or — for a revision made by
//! hand — the same one again.
//!
//! A song folder keeps its charts as revisions (ADR-0011): `chart.json`
//! is revision 1, `chart.vN.json` revision N, and `chart-active.json`
//! names the one the game plays. Every save names its target
//! ([`SaveTarget`]):
//!
//! - **a new revision** — the next free number, made active, with
//!   provenance that names its parent and says it was made by hand
//!   ([`beatbyte_chart::versions::EDITOR_DESIGNER`]); the revision it
//!   came from stays on disk untouched;
//! - **overwrite** — the revision being edited is written again. Only
//!   a revision made by hand may be overwritten: a generated one (the
//!   import's chart, a redesign) is what the tools and the blind test
//!   compare against, and it is never replaced.
//!
//! The revision being edited follows the saves: after a new revision
//! is written, THAT is the one an overwrite rewrites.
//!
//! Every write is atomic ([`beatbyte_chart::io::write_atomic`]): a
//! failure leaves the previous file exactly as it was. A chart that
//! does not validate is not written at all.

use std::path::{Path, PathBuf};

use beatbyte_chart::{
    ChartFile, Provenance, Severity, chart_hash, context, io::activate_revision, save_chart_file,
    versions,
};

/// Where a save goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTarget {
    /// The next free revision number, made active.
    NewRevision,
    /// The revision being edited, written again.
    Overwrite,
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// The chart is what is already on disk — nothing written.
    Unchanged,
    /// Written to this file name, which is now the active revision.
    Written {
        /// The file name (`chart.v7.json`).
        name: String,
        /// Its revision number; `None` for a file outside the scheme.
        revision: Option<u32>,
        /// Whether an existing file was written again.
        overwritten: bool,
    },
}

/// The save side of one editing session.
#[derive(Debug, Clone)]
pub struct Saver {
    /// The file being edited: the one opened, then the last one saved.
    editing: PathBuf,
    /// The chart as it is on disk in that file.
    on_disk: ChartFile,
}

impl Saver {
    /// Start a session on the chart loaded from `path`.
    #[must_use]
    pub fn new(path: &Path, opened: ChartFile) -> Saver {
        Saver {
            editing: path.to_path_buf(),
            on_disk: opened,
        }
    }

    /// The file being edited, and where the game loads the chart from
    /// after a save.
    #[must_use]
    pub fn current_path(&self) -> PathBuf {
        self.editing.clone()
    }

    /// The revision number of the file being edited, if it is one.
    #[must_use]
    pub fn current_revision(&self) -> Option<u32> {
        versions::revision_number(&self.file_name())
    }

    /// The number a new revision would get now, if the file being
    /// edited is in the revision scheme (reads the folder).
    #[must_use]
    pub fn next_revision(&self) -> Option<u32> {
        if !self.versioned() {
            return None;
        }
        let existing: Vec<String> = std::fs::read_dir(self.folder())
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        versions::version_number(&versions::next_version_name(&existing))
    }

    /// Whether the file being edited was made by hand.
    #[must_use]
    pub fn current_is_hand_made(&self) -> bool {
        versions::is_hand_edited(&self.on_disk)
    }

    /// Whether a save may overwrite the file being edited: a revision
    /// made by hand, or a file outside the revision scheme (it has no
    /// other way to be saved).
    #[must_use]
    pub fn can_overwrite(&self) -> bool {
        !self.versioned() || self.current_is_hand_made()
    }

    /// Whether a save may start a new revision: only inside the
    /// revision scheme.
    #[must_use]
    pub fn can_start_revision(&self) -> bool {
        self.versioned()
    }

    /// Whether `chart` differs from what is on disk — by what PLAYS
    /// (`chart_hash`), so a no-op edit that was undone is not a change.
    #[must_use]
    pub fn differs(&self, chart: &ChartFile) -> bool {
        chart_hash(chart) != chart_hash(&self.on_disk)
    }

    fn folder(&self) -> PathBuf {
        self.editing
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    fn file_name(&self) -> String {
        self.editing
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// Whether the file being edited belongs to a versioned folder
    /// (`chart.json` / `chart.vN.json`). Hand-managed files with other
    /// names have no revisions; they are rewritten in place (still
    /// atomically).
    fn versioned(&self) -> bool {
        versions::is_valid_target(&self.file_name())
    }

    /// Save `chart` to `target`.
    ///
    /// # Errors
    /// When the chart does not validate, when `target` is not allowed
    /// here (overwriting a generated revision, a new revision outside
    /// the scheme), or when a write fails — in every case each file on
    /// disk is as it was.
    pub fn save(
        &mut self,
        chart: &ChartFile,
        target: SaveTarget,
        now_ms: u64,
    ) -> Result<Saved, String> {
        if !self.differs(chart) {
            return Ok(Saved::Unchanged);
        }
        match target {
            SaveTarget::Overwrite if !self.can_overwrite() => {
                return Err(format!(
                    "not saved — revision {} was generated, not made by hand; \
                     it stays as it is, save as a new revision",
                    self.current_revision().unwrap_or(1)
                ));
            }
            SaveTarget::NewRevision if !self.can_start_revision() => {
                return Err("not saved — this file has no revisions; overwrite it".to_owned());
            }
            _ => {}
        }
        let errors: Vec<String> = chart
            .validate()
            .into_iter()
            .filter(|issue| issue.severity == Severity::Error)
            .map(|issue| issue.to_string())
            .collect();
        if !errors.is_empty() {
            return Err(format!(
                "not saved — the chart has errors: {}",
                errors.join("; ")
            ));
        }
        // The parent: a new revision descends from the one being
        // edited; an overwrite keeps the parent it already had.
        let parent_hash = match target {
            SaveTarget::NewRevision => chart_hash(&self.on_disk),
            SaveTarget::Overwrite => self
                .on_disk
                .provenance
                .as_ref()
                .map_or_else(|| chart_hash(&self.on_disk), |p| p.parent_hash.clone()),
        };
        let mut out = chart.clone();
        out.provenance = Some(Provenance {
            parent_hash,
            designer: versions::EDITOR_DESIGNER.to_owned(),
            created_ms: now_ms,
            directive: None,
        });

        if !self.versioned() {
            save_chart_file(&self.editing, &out).map_err(|e| e.to_string())?;
            self.on_disk = out;
            return Ok(Saved::Written {
                name: self.file_name(),
                revision: None,
                overwritten: true,
            });
        }

        let folder = self.folder();
        let name = match target {
            SaveTarget::Overwrite => self.file_name(),
            SaveTarget::NewRevision => {
                let existing: Vec<String> = std::fs::read_dir(&folder)
                    .map_err(|e| format!("cannot list `{}`: {e}", folder.display()))?
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                versions::next_version_name(&existing)
            }
        };
        let path = folder.join(&name);
        save_chart_file(&path, &out).map_err(|e| e.to_string())?;
        // The pointer last: until it moves, the game still plays the
        // parent, and a failure here leaves a complete, unused file.
        activate_revision(&folder, &name).map_err(|e| format!("cannot write the pointer: {e}"))?;
        // The analysis beside it, where every note still has one; a
        // sidecar that would describe a moved note as another is not
        // written (`context::carry` refuses), and that costs a reading,
        // not the save.
        // On an overwrite the old sidecar is read before it is replaced;
        // one that cannot be carried no longer matches the new content
        // and is ignored by every reader (they check the chart hash).
        if path != self.editing {
            let _ = std::fs::remove_file(context::context_path(&path));
        }
        let _ = context::carry(&self.editing, &self.on_disk, &path, &out);
        self.editing = path;
        self.on_disk = out;
        Ok(Saved::Written {
            revision: versions::revision_number(&name),
            name,
            overwritten: target == SaveTarget::Overwrite,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use beatbyte_chart::{ChartDef, ChartNote, SongMeta, load_chart_file};
    use beatbyte_core::Difficulty;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bb-editor-save-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn def(difficulty: Difficulty, times: &[f64]) -> ChartDef {
        ChartDef {
            difficulty,
            lanes: 5,
            notes: times
                .iter()
                .map(|t| ChartNote {
                    time: *t,
                    lane: 1,
                    len: 0.0,
                    hopo: false,
                })
                .collect(),
            phrases: vec![],
        }
    }

    fn chart() -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Edit Me".into(),
                artist: "Tests".into(),
                audio: "a.ogg".into(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: None,
                genre: None,
            },
            charts: vec![
                def(Difficulty::Medium, &[1.0, 2.0]),
                def(Difficulty::Expert, &[1.0, 1.5, 2.0]),
            ],
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A folder with `chart.json` and the saver opened on it.
    fn opened(tag: &str) -> (PathBuf, Saver, ChartFile) {
        let dir = scratch(tag);
        let path = dir.join("chart.json");
        save_chart_file(&path, &chart()).unwrap();
        let loaded = load_chart_file(&path).unwrap();
        (dir, Saver::new(&path, loaded.clone()), loaded)
    }

    #[test]
    fn an_unchanged_chart_writes_nothing() {
        let (dir, mut saver, loaded) = opened("unchanged");
        let before = std::fs::read(dir.join("chart.json")).unwrap();
        assert_eq!(
            saver.save(&loaded, SaveTarget::NewRevision, 1).unwrap(),
            Saved::Unchanged
        );
        assert_eq!(files(&dir), vec!["chart.json"]);
        assert_eq!(std::fs::read(dir.join("chart.json")).unwrap(), before);
    }

    /// The edit becomes the next version, made active, marked as made
    /// by hand and bound to its parent; the parent is untouched.
    #[test]
    fn an_edit_is_saved_as_a_new_active_version() {
        let (dir, mut saver, loaded) = opened("new");
        let original = std::fs::read(dir.join("chart.json")).unwrap();
        let mut edited = loaded.clone();
        edited.charts[1].notes.remove(1);
        let saved = saver.save(&edited, SaveTarget::NewRevision, 42).unwrap();
        assert_eq!(
            saved,
            Saved::Written {
                name: "chart.v2.json".to_owned(),
                revision: Some(2),
                overwritten: false
            }
        );
        assert_eq!(
            std::fs::read(dir.join("chart.json")).unwrap(),
            original,
            "the parent changed"
        );
        let pointer = std::fs::read_to_string(dir.join("chart-active.json")).unwrap();
        assert!(pointer.contains("chart.v2.json"), "{pointer}");
        let back = load_chart_file(&dir.join("chart.v2.json")).unwrap();
        assert!(versions::is_hand_edited(&back));
        let provenance = back.provenance.clone().unwrap();
        assert_eq!(provenance.parent_hash, chart_hash(&loaded));
        assert_eq!(provenance.created_ms, 42);
        assert_eq!(
            chart_hash(&back),
            chart_hash(&edited),
            "what was saved is not what was edited"
        );
    }

    /// After a save, the new revision is the one being edited: an
    /// overwrite rewrites IT, and a second new revision takes the next
    /// number.
    #[test]
    fn an_overwrite_rewrites_the_revision_just_saved() {
        let (dir, mut saver, loaded) = opened("again");
        let mut edited = loaded.clone();
        edited.charts[1].notes.remove(0);
        saver.save(&edited, SaveTarget::NewRevision, 1).unwrap();
        assert_eq!(saver.current_revision(), Some(2));
        assert_eq!(saver.next_revision(), Some(3));
        assert!(
            saver.can_overwrite(),
            "a hand-made revision may be overwritten"
        );
        edited.charts[1].notes.remove(0);
        let saved = saver.save(&edited, SaveTarget::Overwrite, 2).unwrap();
        assert_eq!(
            saved,
            Saved::Written {
                name: "chart.v2.json".to_owned(),
                revision: Some(2),
                overwritten: true
            }
        );
        assert!(!dir.join("chart.v3.json").exists());
        let back = load_chart_file(&dir.join("chart.v2.json")).unwrap();
        assert_eq!(back.charts[1].notes.len(), 1);
        // Its parent is still the revision it came from, not itself.
        assert_eq!(back.provenance.unwrap().parent_hash, chart_hash(&loaded));
        // Saved again without a change: nothing.
        assert_eq!(
            saver.save(&edited, SaveTarget::Overwrite, 3).unwrap(),
            Saved::Unchanged
        );
    }

    #[test]
    fn a_second_new_revision_takes_the_next_number_and_descends_from_the_first() {
        let (dir, mut saver, loaded) = opened("twice");
        let mut edited = loaded.clone();
        edited.charts[1].notes.remove(0);
        saver.save(&edited, SaveTarget::NewRevision, 1).unwrap();
        let first = load_chart_file(&dir.join("chart.v2.json")).unwrap();
        edited.charts[1].notes.remove(0);
        let saved = saver.save(&edited, SaveTarget::NewRevision, 2).unwrap();
        assert!(
            matches!(saved, Saved::Written { ref name, revision: Some(3), overwritten: false } if name == "chart.v3.json"),
            "{saved:?}"
        );
        let second = load_chart_file(&dir.join("chart.v3.json")).unwrap();
        assert_eq!(second.provenance.unwrap().parent_hash, chart_hash(&first));
        let pointer = std::fs::read_to_string(dir.join("chart-active.json")).unwrap();
        assert!(pointer.contains("chart.v3.json"), "{pointer}");
        assert!(
            dir.join("chart.v2.json").exists(),
            "the first revision stays"
        );
    }

    /// ⚠️ A generated revision is never replaced: the tools and the
    /// blind test compare against it.
    #[test]
    fn a_generated_revision_is_never_overwritten() {
        let (dir, mut saver, loaded) = opened("generated");
        assert!(!saver.can_overwrite());
        assert!(saver.can_start_revision());
        assert_eq!(saver.current_revision(), Some(1));
        let before = std::fs::read(dir.join("chart.json")).unwrap();
        let mut edited = loaded.clone();
        edited.charts[1].notes.remove(0);
        let error = saver.save(&edited, SaveTarget::Overwrite, 1).unwrap_err();
        assert!(error.contains("generated"), "{error}");
        assert_eq!(files(&dir), vec!["chart.json"]);
        assert_eq!(std::fs::read(dir.join("chart.json")).unwrap(), before);
    }

    /// A session opened on a hand-made revision may overwrite it.
    #[test]
    fn a_hand_made_revision_opened_later_can_be_overwritten() {
        let (dir, mut first, loaded) = opened("reopen");
        let mut edited = loaded.clone();
        edited.charts[1].notes.remove(0);
        first.save(&edited, SaveTarget::NewRevision, 1).unwrap();
        let path = dir.join("chart.v2.json");
        let mut saver = Saver::new(&path, load_chart_file(&path).unwrap());
        assert!(saver.current_is_hand_made() && saver.can_overwrite());
        edited.charts[0].notes.clear();
        saver.save(&edited, SaveTarget::Overwrite, 2).unwrap();
        assert!(load_chart_file(&path).unwrap().charts[0].notes.is_empty());
        assert!(!dir.join("chart.v3.json").exists());
    }

    /// ⚠️ Editing one difficulty leaves every other one exactly as it
    /// was on disk.
    #[test]
    fn other_difficulties_are_saved_untouched() {
        let (dir, mut saver, loaded) = opened("others");
        let mut edited = loaded.clone();
        edited.charts[1].notes[0].time = 0.75;
        saver.save(&edited, SaveTarget::NewRevision, 1).unwrap();
        let back = load_chart_file(&dir.join("chart.v2.json")).unwrap();
        assert_eq!(
            back.chart_for(Difficulty::Medium),
            loaded.chart_for(Difficulty::Medium)
        );
        assert_ne!(
            back.chart_for(Difficulty::Expert),
            loaded.chart_for(Difficulty::Expert)
        );
        assert_eq!(back.song, loaded.song, "the song block moved");
        assert_eq!(back.grid, loaded.grid);
        assert_eq!(back.audio_trim, loaded.audio_trim);
    }

    #[test]
    fn an_invalid_chart_is_not_written() {
        let (dir, mut saver, loaded) = opened("invalid");
        let mut edited = loaded.clone();
        edited.charts[1].notes[0].lane = 9;
        let error = saver.save(&edited, SaveTarget::NewRevision, 1).unwrap_err();
        assert!(error.contains("errors"), "{error}");
        assert_eq!(files(&dir), vec!["chart.json"]);
    }

    /// A version made on top of existing versions takes the next free
    /// number; it never reuses one.
    #[test]
    fn the_next_free_version_is_taken() {
        let (dir, _, loaded) = opened("free");
        save_chart_file(&dir.join("chart.v2.json"), &loaded).unwrap();
        save_chart_file(&dir.join("chart.v5.json"), &loaded).unwrap();
        let path = dir.join("chart.v5.json");
        let mut saver = Saver::new(&path, load_chart_file(&path).unwrap());
        let mut edited = loaded.clone();
        edited.charts[0].notes.clear();
        let saved = saver.save(&edited, SaveTarget::NewRevision, 1).unwrap();
        assert!(
            matches!(saved, Saved::Written { ref name, .. } if name == "chart.v6.json"),
            "{saved:?}"
        );
        assert_eq!(saver.current_path(), dir.join("chart.v6.json"));
    }

    /// A hand-managed file outside the version scheme is rewritten in
    /// place.
    #[test]
    fn a_file_outside_the_scheme_is_rewritten_in_place() {
        let dir = scratch("inplace");
        let path = dir.join("my-song.chart.json");
        save_chart_file(&path, &chart()).unwrap();
        let mut saver = Saver::new(&path, load_chart_file(&path).unwrap());
        let mut edited = chart();
        edited.charts[0].notes.clear();
        assert!(saver.can_overwrite() && !saver.can_start_revision());
        assert!(saver.save(&edited, SaveTarget::NewRevision, 1).is_err());
        saver.save(&edited, SaveTarget::Overwrite, 1).unwrap();
        assert_eq!(files(&dir), vec!["my-song.chart.json"]);
        assert!(load_chart_file(&path).unwrap().charts[0].notes.is_empty());
    }
}
