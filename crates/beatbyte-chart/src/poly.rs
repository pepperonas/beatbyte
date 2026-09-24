//! Which notes were struck together: the polyphony sidecar.
//!
//! `<audio stem>.poly.json` beside a song holds the notes a polyphonic
//! transcription heard (`beatbyte-poly`, Basic Pitch) — start, end,
//! pitch and strength — and what it heard them in. It is the evidence
//! the classic `chords` ingredient reads: a chord is written where the
//! recording has several notes STRUCK at that moment, and nowhere
//! else. This crate only reads and writes the file; making one needs
//! the model and lives in the command line.
//!
//! It is untrusted input like a chart: the size, every number and the
//! note count are checked on the way in, and a file that fails any of
//! it is refused rather than half used.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::io::ChartIoError;

/// The sidecar's format tag.
pub const POLY_FORMAT: &str = "beatbyte.poly/1";

/// The largest sidecar read: an hour of dense music is well under it.
pub const MAX_POLY_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// The most notes one sidecar may hold.
pub const MAX_POLY_NOTES: usize = 200_000;

/// One transcribed note, in the file's short field names.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PolyNote {
    /// Start, seconds on the song timeline.
    #[serde(rename = "s")]
    pub start_s: f64,
    /// End, seconds.
    #[serde(rename = "e")]
    pub end_s: f64,
    /// Pitch, MIDI.
    #[serde(rename = "m")]
    pub midi: u8,
    /// How strongly it sounded, `0`–`1`.
    #[serde(rename = "a")]
    pub amplitude: f32,
}

/// A song's polyphony sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolyFile {
    /// [`POLY_FORMAT`].
    pub format: String,
    /// The model's registry id.
    pub model: String,
    /// The model's SHA-256: which weights heard this.
    pub model_sha256: String,
    /// What was transcribed — the separated stem it was read from, or
    /// the mix. Basic Pitch hears one instrument best, so this says
    /// how far the notes can be trusted as the guitar's.
    pub source: String,
    /// The notes, by start time.
    pub notes: Vec<PolyNote>,
}

impl PolyFile {
    /// Why this file cannot be used, or `None`.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        if self.format != POLY_FORMAT {
            return Some(format!("format `{}` is not `{POLY_FORMAT}`", self.format));
        }
        if self.notes.len() > MAX_POLY_NOTES {
            return Some(format!(
                "{} notes, more than {MAX_POLY_NOTES}",
                self.notes.len()
            ));
        }
        if self.model.len() > 64 || self.model_sha256.len() > 64 || self.source.len() > 128 {
            return Some("a label is too long".to_owned());
        }
        for (index, note) in self.notes.iter().enumerate() {
            let sane = note.start_s.is_finite()
                && note.end_s.is_finite()
                && note.start_s >= -1.0
                && note.end_s > note.start_s
                && note.end_s <= crate::MAX_SONG_LENGTH_S
                && note.midi <= 127
                && note.amplitude.is_finite()
                && (0.0..=1.0).contains(&note.amplitude);
            if !sane {
                return Some(format!("note {index} is not a note: {note:?}"));
            }
        }
        None
    }

    /// The notes that START within `window_s` of `time_s`: what was
    /// struck at that moment.
    #[must_use]
    pub fn struck_at(&self, time_s: f64, window_s: f64) -> Vec<PolyNote> {
        let from = self
            .notes
            .partition_point(|n| n.start_s < time_s - window_s);
        self.notes[from..]
            .iter()
            .take_while(|n| n.start_s <= time_s + window_s)
            .copied()
            .collect()
    }
}

/// Where a song's polyphony sidecar lives: `<audio stem>.poly.json`.
#[must_use]
pub fn poly_path(audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    audio_path.with_file_name(format!("{stem}.poly.json"))
}

/// Read and check a sidecar.
///
/// # Errors
/// When it cannot be read, is too large, does not parse, or fails
/// [`PolyFile::problem`] (reported as a parse error naming it).
pub fn load_poly(path: &Path) -> Result<PolyFile, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size > MAX_POLY_FILE_BYTES {
        return Err(format!(
            "{}: {size} bytes, more than {MAX_POLY_FILE_BYTES}",
            path.display()
        ));
    }
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut file: PolyFile =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    if let Some(problem) = file.problem() {
        return Err(format!("{}: {problem}", path.display()));
    }
    // Sorted on the way in, whatever order the file was in: every
    // lookup is a binary search.
    file.notes
        .sort_by(|a, b| a.start_s.total_cmp(&b.start_s).then(a.midi.cmp(&b.midi)));
    Ok(file)
}

/// Write a sidecar through `.part` and a rename, so a crash leaves the
/// old file or none.
///
/// # Errors
/// When the file cannot be written.
pub fn save_poly(path: &Path, file: &PolyFile) -> Result<(), ChartIoError> {
    let io_err = |source| ChartIoError::Io {
        path: path.to_path_buf(),
        source,
    };
    let json = serde_json::to_string(file).map_err(|source| ChartIoError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let part = crate::vocals::part_path(path);
    std::fs::write(&part, json).map_err(io_err)?;
    std::fs::rename(&part, path).map_err(|source| {
        let _ = std::fs::remove_file(&part);
        ChartIoError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start_s: f64, midi: u8) -> PolyNote {
        PolyNote {
            start_s,
            end_s: start_s + 0.5,
            midi,
            amplitude: 0.7,
        }
    }

    fn file(notes: Vec<PolyNote>) -> PolyFile {
        PolyFile {
            format: POLY_FORMAT.to_owned(),
            model: "basic-pitch".to_owned(),
            model_sha256: "2c3c".to_owned(),
            source: "other stem (htdemucs)".to_owned(),
            notes,
        }
    }

    #[test]
    fn the_path_sits_beside_the_audio() {
        assert_eq!(
            poly_path(Path::new("/s/Band - Song.m4a")),
            PathBuf::from("/s/Band - Song.poly.json")
        );
    }

    #[test]
    fn struck_at_finds_the_notes_that_start_near_a_moment() {
        let f = file(vec![
            note(0.9, 40),
            note(1.0, 45),
            note(1.02, 52),
            note(1.5, 40),
        ]);
        let midi = |notes: Vec<PolyNote>| notes.iter().map(|n| n.midi).collect::<Vec<_>>();
        assert_eq!(midi(f.struck_at(1.0, 0.05)), vec![45, 52]);
        assert_eq!(midi(f.struck_at(1.0, 0.1)), vec![40, 45, 52]);
        assert!(f.struck_at(3.0, 0.05).is_empty());
    }

    #[test]
    fn a_round_trip_keeps_every_note_and_sorts_them() {
        let dir = std::env::temp_dir().join(format!(
            "bb-poly-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join("a.poly.json");
        let written = file(vec![note(2.0, 50), note(1.0, 40)]);
        save_poly(&path, &written).expect("written");
        assert!(
            !crate::vocals::part_path(&path).exists(),
            "the .part was left"
        );
        let read = load_poly(&path).expect("read");
        assert_eq!(read.notes, vec![note(1.0, 40), note(2.0, 50)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Untrusted input: every field is checked, and a file that fails
    /// is refused whole.
    #[test]
    fn a_sidecar_that_is_not_sane_is_refused() {
        assert_eq!(file(vec![note(1.0, 40)]).problem(), None);
        let mut wrong = file(vec![]);
        wrong.format = "beatbyte.poly/9".to_owned();
        assert!(wrong.problem().is_some());
        for broken in [
            PolyNote {
                end_s: 0.5,
                ..note(1.0, 40)
            },
            PolyNote {
                start_s: f64::NAN,
                ..note(1.0, 40)
            },
            PolyNote {
                midi: 128,
                ..note(1.0, 40)
            },
            PolyNote {
                amplitude: 1.5,
                ..note(1.0, 40)
            },
        ] {
            assert!(file(vec![broken]).problem().is_some(), "{broken:?} passed");
        }
        let many = file(vec![note(1.0, 40); MAX_POLY_NOTES + 1]);
        assert!(many.problem().is_some());
    }
}
