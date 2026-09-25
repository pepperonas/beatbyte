//! Saving an edit: a NEW chart version, never an overwrite.
//!
//! A song folder keeps its charts as versions (ADR-0011): `chart.json`
//! is the original, `chart.vN.json` are later ones and
//! `chart-active.json` names the one the game plays. An edit is saved
//! the same way — as the next version, made active, with provenance
//! that names its parent and says it was made by hand
//! ([`beatbyte_chart::versions::EDITOR_DESIGNER`]). The version it was
//! made from stays on disk untouched, so every edit can be taken back
//! by pointing at the older file, and the tools that write versions
//! know to leave a hand-made one active.
//!
//! One editing SESSION writes one version: the first save creates it,
//! later saves in the same session rewrite it. Twenty presses of S
//! are one piece of work, not twenty versions.
//!
//! Every write is atomic ([`beatbyte_chart::io::write_atomic`]): a
//! failure leaves the previous file exactly as it was. A chart that
//! does not validate is not written at all.

use std::path::{Path, PathBuf};

use beatbyte_chart::{
    ChartFile, Provenance, Severity, chart_hash, context, io::write_atomic, save_chart_file,
    versions,
};

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// The chart is what is already on disk — nothing written.
    Unchanged,
    /// Written to this file name, which is now the active version.
    Written {
        /// The version's file name (`chart.v7.json`).
        name: String,
        /// Whether the file already existed from an earlier save in
        /// this session.
        rewritten: bool,
    },
}

/// The save side of one editing session.
#[derive(Debug, Clone)]
pub struct Saver {
    /// The file the session was opened from.
    opened_path: PathBuf,
    /// The chart as it was opened — the parent of whatever is saved.
    opened: ChartFile,
    /// The chart as it is on disk now (after the last save).
    on_disk: ChartFile,
    /// The version this session has written, once it has.
    written: Option<String>,
}

impl Saver {
    /// Start a session on the chart loaded from `path`.
    #[must_use]
    pub fn new(path: &Path, opened: ChartFile) -> Saver {
        Saver {
            opened_path: path.to_path_buf(),
            on_disk: opened.clone(),
            opened,
            written: None,
        }
    }

    /// The file the next save writes, and where the game will then
    /// load the chart from.
    #[must_use]
    pub fn current_path(&self) -> PathBuf {
        match &self.written {
            Some(name) => self.folder().join(name),
            None => self.opened_path.clone(),
        }
    }

    /// Whether `chart` differs from what is on disk — by what PLAYS
    /// (`chart_hash`), so a no-op edit that was undone is not a change.
    #[must_use]
    pub fn differs(&self, chart: &ChartFile) -> bool {
        chart_hash(chart) != chart_hash(&self.on_disk)
    }

    fn folder(&self) -> PathBuf {
        self.opened_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    /// Whether the opened file belongs to a versioned folder
    /// (`chart.json` / `chart.vN.json`). Hand-managed files with other
    /// names have no versions; they are rewritten in place (still
    /// atomically).
    fn versioned(&self) -> bool {
        self.opened_path
            .file_name()
            .map(|n| n.to_string_lossy())
            .is_some_and(|name| versions::is_valid_target(&name))
    }

    /// Save `chart`.
    ///
    /// # Errors
    /// When the chart does not validate, or a write fails — in both
    /// cases every file on disk is as it was.
    pub fn save(&mut self, chart: &ChartFile, now_ms: u64) -> Result<Saved, String> {
        if !self.differs(chart) {
            return Ok(Saved::Unchanged);
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
        let mut out = chart.clone();
        out.provenance = Some(Provenance {
            parent_hash: chart_hash(&self.opened),
            designer: versions::EDITOR_DESIGNER.to_owned(),
            created_ms: now_ms,
            directive: None,
        });

        if !self.versioned() {
            save_chart_file(&self.opened_path, &out).map_err(|e| e.to_string())?;
            self.on_disk = out;
            let name = self
                .opened_path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let rewritten = self.written.replace(name.clone()).is_some();
            return Ok(Saved::Written { name, rewritten });
        }

        let folder = self.folder();
        let (name, rewritten) = match &self.written {
            Some(name) => (name.clone(), true),
            None => {
                let existing: Vec<String> = std::fs::read_dir(&folder)
                    .map_err(|e| format!("cannot list `{}`: {e}", folder.display()))?
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                (versions::next_version_name(&existing), false)
            }
        };
        let path = folder.join(&name);
        save_chart_file(&path, &out).map_err(|e| e.to_string())?;
        // The pointer last: until it moves, the game still plays the
        // parent, and a failure here leaves a complete, unused version.
        write_atomic(
            &folder.join(versions::POINTER_FILE),
            format!("{{\"active\": \"{name}\"}}\n").as_bytes(),
        )
        .map_err(|e| format!("cannot write the pointer: {e}"))?;
        // The analysis beside it, where every note still has one; a
        // sidecar that would describe a moved note as another is not
        // written (`context::carry` refuses), and that costs a reading,
        // not the save.
        let _ = std::fs::remove_file(context::context_path(&path));
        let _ = context::carry(&self.opened_path, &self.opened, &path, &out);
        self.written = Some(name.clone());
        self.on_disk = out;
        Ok(Saved::Written { name, rewritten })
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
        assert_eq!(saver.save(&loaded, 1).unwrap(), Saved::Unchanged);
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
        let saved = saver.save(&edited, 42).unwrap();
        assert_eq!(
            saved,
            Saved::Written {
                name: "chart.v2.json".to_owned(),
                rewritten: false
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

    /// One session, one version: saving again rewrites it.
    #[test]
    fn a_second_save_in_the_session_rewrites_its_version() {
        let (dir, mut saver, loaded) = opened("again");
        let mut edited = loaded.clone();
        edited.charts[1].notes.remove(0);
        saver.save(&edited, 1).unwrap();
        edited.charts[1].notes.remove(0);
        let saved = saver.save(&edited, 2).unwrap();
        assert_eq!(
            saved,
            Saved::Written {
                name: "chart.v2.json".to_owned(),
                rewritten: true
            }
        );
        assert!(!dir.join("chart.v3.json").exists());
        let back = load_chart_file(&dir.join("chart.v2.json")).unwrap();
        assert_eq!(back.charts[1].notes.len(), 1);
        // Saved again without a change: nothing.
        assert_eq!(saver.save(&edited, 3).unwrap(), Saved::Unchanged);
    }

    /// ⚠️ Editing one difficulty leaves every other one exactly as it
    /// was on disk.
    #[test]
    fn other_difficulties_are_saved_untouched() {
        let (dir, mut saver, loaded) = opened("others");
        let mut edited = loaded.clone();
        edited.charts[1].notes[0].time = 0.75;
        saver.save(&edited, 1).unwrap();
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
        let error = saver.save(&edited, 1).unwrap_err();
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
        let saved = saver.save(&edited, 1).unwrap();
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
        saver.save(&edited, 1).unwrap();
        assert_eq!(files(&dir), vec!["my-song.chart.json"]);
        assert!(load_chart_file(&path).unwrap().charts[0].notes.is_empty());
    }
}
