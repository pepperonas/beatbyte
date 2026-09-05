//! Measuring the aligner against word-level ground truth (plan
//! milestone L5) — the metrics the field publishes, so the numbers
//! compare against papers instead of against feel:
//!
//! - **AAE**, average absolute error between predicted and true word
//!   onset, in seconds;
//! - **PCO@τ**, the share of words whose predicted onset lies within
//!   τ of the true one — reported at 0.3 s (the standard, too loose
//!   for a fill) and 0.1 s (what decides whether the fill looks glued
//!   to the voice).
//!
//! Everything here is pure — predictions and truth in, scores out —
//! and unit-tested on cases whose answers are worked out by hand.
//! The corpus reader ([`JamendoCorpus`]) knows the **JamendoLyrics
//! MultiLang** layout (80 songs, 20 each in English, German, Spanish
//! and French, word timestamps for every word); the corpus itself
//! never enters this repository (CC licences with NC terms on part
//! of it), it is pointed at through `BEATBYTE_LYRICS_CORPUS`.
//!
//! Words are paired by their text, in order (a longest common
//! subsequence over normalised words), because the corpus's word list
//! and this crate's transcript can disagree on a token here and there
//! (`don't` vs `don t`); the share of truth words that found a
//! partner is reported as coverage, so a low number can never hide.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::transcript::tokens_of;

/// The gates from plan §2: below these, the regression test fails.
pub const GATE_AAE_S: f64 = 0.30;
/// PCO@0.3 s must exceed this.
pub const GATE_PCO_03: f64 = 0.80;
/// PCO@0.1 s must exceed this.
pub const GATE_PCO_01: f64 = 0.55;

/// One word of ground truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TruthWord {
    /// The word as the corpus writes it.
    pub text: String,
    /// True onset, seconds.
    pub start: f64,
    /// True offset, seconds.
    pub end: f64,
}

/// One predicted word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredWord {
    /// The word as the transcript wrote it.
    pub text: String,
    /// Predicted onset, seconds.
    pub start: f64,
    /// Whether the pipeline marked it estimated (no acoustic
    /// evidence of its own).
    pub estimated: bool,
}

/// The scores of one song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongScore {
    /// The song's name in the corpus.
    pub song: String,
    /// Its language.
    pub language: String,
    /// Truth words.
    pub words_truth: usize,
    /// Truth words that found a predicted partner.
    pub words_matched: usize,
    /// Share of truth words matched, 0..1.
    pub coverage: f64,
    /// Average absolute onset error over matched words, seconds
    /// (infinite when nothing matched — JSON writes that as `null`
    /// and reads it back as infinite).
    #[serde(deserialize_with = "null_is_infinite")]
    pub aae_s: f64,
    /// Share of matched words within 0.1 s.
    pub pco_01: f64,
    /// Share of matched words within 0.3 s.
    pub pco_03: f64,
    /// Share of predicted words the pipeline marked estimated.
    pub estimated_rate: f64,
}

/// serde_json writes a non-finite float as `null`; read it back as
/// the infinity it was, so a song that matched nothing stays a
/// failed song after a round trip.
fn null_is_infinite<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::INFINITY))
}

/// Scores over a set of songs — the mean of the per-song numbers
/// (the field's convention: every song weighs the same, a long song
/// does not drown a short one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aggregate {
    /// Songs in the set.
    pub songs: usize,
    /// Mean AAE, seconds.
    #[serde(deserialize_with = "null_is_infinite")]
    pub aae_s: f64,
    /// Mean PCO@0.1.
    pub pco_01: f64,
    /// Mean PCO@0.3.
    pub pco_03: f64,
    /// Mean coverage.
    pub coverage: f64,
    /// Mean estimated rate.
    pub estimated_rate: f64,
}

impl Aggregate {
    /// Whether the plan's gates hold.
    #[must_use]
    pub fn passes_gates(&self) -> bool {
        self.songs > 0
            && self.aae_s < GATE_AAE_S
            && self.pco_03 > GATE_PCO_03
            && self.pco_01 > GATE_PCO_01
    }
}

/// The whole report, as `lyrics-eval --out` writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// `beatbyte.lyrics-eval/1`.
    pub schema: String,
    /// The aligner: model id, hash, runtime fingerprint.
    pub aligner: String,
    /// Whether the confidence gate ran.
    pub gated: bool,
    /// Every song.
    pub songs: Vec<SongScore>,
    /// Per language, keyed by the corpus's language name.
    pub by_language: Vec<(String, Aggregate)>,
    /// Everything.
    pub all: Aggregate,
}

/// The report schema.
pub const REPORT_SCHEMA: &str = "beatbyte.lyrics-eval/1";

/// A word as it is compared: the model's letters only, so `Don't`,
/// `don't` and `dont` pair up.
fn key(text: &str) -> Vec<u8> {
    tokens_of(text)
}

/// Pair predictions with truth by text, in order: the longest common
/// subsequence over normalised words. Returns `(pred index, truth
/// index)` pairs. Pure — tested.
#[must_use]
pub fn pair_words(pred: &[PredWord], truth: &[TruthWord]) -> Vec<(usize, usize)> {
    let a: Vec<Vec<u8>> = pred.iter().map(|w| key(&w.text)).collect();
    let b: Vec<Vec<u8>> = truth.iter().map(|w| key(&w.text)).collect();
    let (n, m) = (a.len(), b.len());
    // Classic O(n·m) LCS table; a song is a few hundred words.
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if !a[i].is_empty() && a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if !a[i].is_empty() && a[i] == b[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

/// Score one song. Pure — tested.
#[must_use]
pub fn score(song: &str, language: &str, pred: &[PredWord], truth: &[TruthWord]) -> SongScore {
    let pairs = pair_words(pred, truth);
    let errors: Vec<f64> = pairs
        .iter()
        .map(|&(p, t)| (pred[p].start - truth[t].start).abs())
        .collect();
    let matched = errors.len();
    let share = |tol: f64| {
        if matched == 0 {
            0.0
        } else {
            // Inclusive at the tolerance (an error of exactly 0.1 s IS
            // within 0.1 s), with a hair of slack for float arithmetic.
            errors.iter().filter(|e| **e <= tol + 1e-9).count() as f64 / matched as f64
        }
    };
    SongScore {
        song: song.to_owned(),
        language: language.to_owned(),
        words_truth: truth.len(),
        words_matched: matched,
        coverage: if truth.is_empty() {
            0.0
        } else {
            matched as f64 / truth.len() as f64
        },
        aae_s: if matched == 0 {
            f64::INFINITY
        } else {
            errors.iter().sum::<f64>() / matched as f64
        },
        pco_01: share(0.1),
        pco_03: share(0.3),
        estimated_rate: if pred.is_empty() {
            0.0
        } else {
            pred.iter().filter(|w| w.estimated).count() as f64 / pred.len() as f64
        },
    }
}

/// The mean of per-song scores. Pure — tested.
#[must_use]
pub fn aggregate(scores: &[SongScore]) -> Aggregate {
    let n = scores.len() as f64;
    let mean = |f: fn(&SongScore) -> f64| {
        if scores.is_empty() {
            0.0
        } else {
            scores.iter().map(f).sum::<f64>() / n
        }
    };
    Aggregate {
        songs: scores.len(),
        aae_s: mean(|s| s.aae_s),
        pco_01: mean(|s| s.pco_01),
        pco_03: mean(|s| s.pco_03),
        coverage: mean(|s| s.coverage),
        estimated_rate: mean(|s| s.estimated_rate),
    }
}

/// Group and aggregate by language, in first-seen order.
#[must_use]
pub fn by_language(scores: &[SongScore]) -> Vec<(String, Aggregate)> {
    let mut languages: Vec<String> = Vec::new();
    for s in scores {
        if !languages.contains(&s.language) {
            languages.push(s.language.clone());
        }
    }
    languages
        .into_iter()
        .map(|language| {
            let subset: Vec<SongScore> = scores
                .iter()
                .filter(|s| s.language == language)
                .cloned()
                .collect();
            (language, aggregate(&subset))
        })
        .collect()
}

/// One song of the corpus, located.
#[derive(Debug, Clone, PartialEq)]
pub struct CorpusSong {
    /// The corpus's name for it (the `Filepath` stem).
    pub name: String,
    /// `Language` from the metadata.
    pub language: String,
    /// The audio file.
    pub audio: PathBuf,
    /// The normalised lyrics text file (what the aligner reads).
    pub lyrics: PathBuf,
    /// The word annotations, in order.
    pub words: Vec<TruthWord>,
}

/// The JamendoLyrics MultiLang corpus on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct JamendoCorpus {
    /// Its root (the directory holding `JamendoLyrics.csv`).
    pub root: PathBuf,
    /// Every song the metadata lists whose files are all present.
    pub songs: Vec<CorpusSong>,
    /// Songs listed but missing a file, with the reason.
    pub skipped: Vec<(String, String)>,
}

impl JamendoCorpus {
    /// Read the corpus at `root`. Layout (verified against the
    /// published repository, 2026-09-05):
    ///
    /// ```text
    /// JamendoLyrics.csv        URL,Filepath,Artist,Title,Genre,LicenseType,Language,…
    /// mp3/<Filepath>
    /// lyrics/<stem>.txt        normalised lyrics
    /// lyrics/<stem>.words.txt  one word per line, the annotation order
    /// annotations/words/<stem>.csv   word_start,word_end,line_end
    /// ```
    pub fn load(root: &Path) -> Result<JamendoCorpus, String> {
        let meta_path = root.join("JamendoLyrics.csv");
        let meta = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("cannot read `{}`: {e}", meta_path.display()))?;
        let mut lines = meta.lines();
        let header = csv_split(lines.next().unwrap_or_default());
        let column = |name: &str| header.iter().position(|h| h == name);
        let (Some(file_col), Some(lang_col)) = (column("Filepath"), column("Language")) else {
            return Err(format!(
                "`{}` lacks the Filepath/Language columns: {header:?}",
                meta_path.display()
            ));
        };
        let mut songs = Vec::new();
        let mut skipped = Vec::new();
        for line in lines.filter(|l| !l.trim().is_empty()) {
            let fields = csv_split(line);
            let (Some(file), Some(language)) = (fields.get(file_col), fields.get(lang_col)) else {
                continue;
            };
            let stem = Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.clone());
            let audio = root.join("mp3").join(file);
            let lyrics = root.join("lyrics").join(format!("{stem}.txt"));
            let word_list = root.join("lyrics").join(format!("{stem}.words.txt"));
            let annotations = root
                .join("annotations")
                .join("words")
                .join(format!("{stem}.csv"));
            let mut missing = Vec::new();
            for (what, path) in [
                ("audio", &audio),
                ("lyrics", &lyrics),
                ("word list", &word_list),
                ("word annotations", &annotations),
            ] {
                if !path.is_file() {
                    missing.push(format!("{what} `{}`", path.display()));
                }
            }
            if !missing.is_empty() {
                skipped.push((stem, missing.join(", ")));
                continue;
            }
            let words = match read_word_annotations(&word_list, &annotations) {
                Ok(words) => words,
                Err(reason) => {
                    skipped.push((stem, reason));
                    continue;
                }
            };
            songs.push(CorpusSong {
                name: stem,
                language: language.clone(),
                audio,
                lyrics,
                words,
            });
        }
        Ok(JamendoCorpus {
            root: root.to_path_buf(),
            songs,
            skipped,
        })
    }
}

/// The word list and the annotation CSV, zipped: one truth word per
/// row. Pure over the two texts — tested.
pub fn parse_word_annotations(
    word_list: &str,
    annotations: &str,
) -> Result<Vec<TruthWord>, String> {
    let words: Vec<&str> = word_list
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect();
    let mut rows = annotations.lines().filter(|l| !l.trim().is_empty());
    let header = csv_split(rows.next().unwrap_or_default());
    let (Some(start_col), Some(end_col)) = (
        header.iter().position(|h| h == "word_start"),
        header.iter().position(|h| h == "word_end"),
    ) else {
        return Err(format!(
            "annotation header lacks word_start/word_end: {header:?}"
        ));
    };
    let mut out = Vec::with_capacity(words.len());
    for (index, row) in rows.enumerate() {
        let fields = csv_split(row);
        let time = |col: usize| -> Result<f64, String> {
            fields
                .get(col)
                .and_then(|f| f.trim().parse::<f64>().ok())
                .filter(|t| t.is_finite() && *t >= 0.0)
                .ok_or_else(|| format!("row {}: bad time in {row:?}", index + 2))
        };
        let Some(text) = words.get(index) else {
            return Err(format!(
                "{} annotation rows but {} words in the word list",
                index + 1,
                words.len()
            ));
        };
        out.push(TruthWord {
            text: (*text).to_owned(),
            start: time(start_col)?,
            end: time(end_col)?,
        });
    }
    if out.len() != words.len() {
        return Err(format!(
            "{} annotation rows but {} words in the word list",
            out.len(),
            words.len()
        ));
    }
    Ok(out)
}

fn read_word_annotations(word_list: &Path, annotations: &Path) -> Result<Vec<TruthWord>, String> {
    let words = std::fs::read_to_string(word_list)
        .map_err(|e| format!("cannot read `{}`: {e}", word_list.display()))?;
    let csv = std::fs::read_to_string(annotations)
        .map_err(|e| format!("cannot read `{}`: {e}", annotations.display()))?;
    parse_word_annotations(&words, &csv)
}

/// Align one corpus song and score it — the whole pipeline the game
/// runs (`gated`), or the raw aligner. The predictions are the
/// aligned words' onsets, in transcript order.
pub fn evaluate_song(
    song: &CorpusSong,
    runtime: &beatbyte_ml::Runtime,
    model: &beatbyte_ml::Loaded,
    gated: bool,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<SongScore, String> {
    let lyrics = std::fs::read_to_string(&song.lyrics)
        .map_err(|e| format!("cannot read `{}`: {e}", song.lyrics.display()))?;
    let transcript = crate::transcript::Transcript::parse(&lyrics);
    if transcript.alignable_words() == 0 {
        return Err("no alignable words".to_owned());
    }
    let audio = beatbyte_audio::decode_file(&song.audio).map_err(|e| e.to_string())?;
    let mut outcome = crate::align::align_with(
        &audio,
        "",
        &transcript,
        &format!("corpus:{}", song.name),
        runtime,
        model,
        &mut |_| {},
        cancel,
    )
    .map_err(|e| e.to_string())?;
    if gated {
        crate::gate::gate(
            &mut outcome.alignment,
            &transcript,
            audio.duration_s(),
            &crate::gate::GateConfig::default(),
        );
    }
    let pred: Vec<PredWord> = outcome
        .alignment
        .words()
        .map(|w| PredWord {
            text: w.text.clone(),
            start: w.start,
            estimated: w.estimated,
        })
        .collect();
    Ok(score(&song.name, &song.language, &pred, &song.words))
}

/// A minimal quote-aware CSV split (the metadata's titles may carry
/// commas inside quotes). Pure — tested.
#[must_use]
pub fn csv_split(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    fields.push(field);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn truth(words: &[(&str, f64)]) -> Vec<TruthWord> {
        words
            .iter()
            .map(|(t, s)| TruthWord {
                text: (*t).to_owned(),
                start: *s,
                end: s + 0.2,
            })
            .collect()
    }

    fn pred(words: &[(&str, f64)]) -> Vec<PredWord> {
        words
            .iter()
            .map(|(t, s)| PredWord {
                text: (*t).to_owned(),
                start: *s,
                estimated: false,
            })
            .collect()
    }

    #[test]
    fn the_metrics_are_what_the_field_means_by_them() {
        // Four words: errors 0.05, 0.15, 0.25, 0.50.
        let t = truth(&[("a", 1.0), ("b", 2.0), ("c", 3.0), ("d", 4.0)]);
        let p = pred(&[("a", 1.05), ("b", 2.15), ("c", 2.75), ("d", 4.5)]);
        let s = score("x", "English", &p, &t);
        assert_eq!((s.words_truth, s.words_matched), (4, 4));
        assert!((s.aae_s - 0.2375).abs() < 1e-9, "{}", s.aae_s);
        assert!(
            (s.pco_01 - 0.25).abs() < 1e-9,
            "one within 0.1: {}",
            s.pco_01
        );
        assert!(
            (s.pco_03 - 0.75).abs() < 1e-9,
            "three within 0.3: {}",
            s.pco_03
        );
        assert!((s.coverage - 1.0).abs() < 1e-9);
        // The tolerance is inclusive: exactly 0.1 s counts at 0.1.
        let t = truth(&[("a", 1.0), ("b", 2.0)]);
        let p = pred(&[("a", 1.1), ("b", 2.3)]);
        let s = score("x", "English", &p, &t);
        assert!((s.pco_01 - 0.5).abs() < 1e-9, "{}", s.pco_01);
        assert!((s.pco_03 - 1.0).abs() < 1e-9, "{}", s.pco_03);
    }

    #[test]
    fn words_pair_by_text_in_order_and_coverage_tells_the_truth() {
        // The transcript split "don't" into two tokens; the corpus
        // has one. Only the words that pair are scored, and the
        // coverage says a word was lost.
        let t = truth(&[("I", 1.0), ("don't", 2.0), ("know", 3.0)]);
        let p = pred(&[("I", 1.0), ("don", 2.0), ("t", 2.1), ("know", 3.1)]);
        let s = score("x", "English", &p, &t);
        assert_eq!(s.words_matched, 2);
        assert!((s.coverage - 2.0 / 3.0).abs() < 1e-9);
        assert!((s.aae_s - 0.05).abs() < 1e-9);
        // Case and apostrophes do not split pairs; an empty key (a
        // number) never pairs.
        let t = truth(&[("Don't", 1.0), ("1999", 2.0), ("go", 3.0)]);
        let p = pred(&[("don't", 1.2), ("1999", 2.2), ("GO", 3.0)]);
        assert_eq!(pair_words(&p, &t), vec![(0, 0), (2, 2)]);
    }

    #[test]
    fn nothing_matched_is_infinite_error_not_a_free_pass() {
        let t = truth(&[("a", 1.0)]);
        let p = pred(&[("b", 1.0)]);
        let s = score("x", "English", &p, &t);
        assert_eq!(s.words_matched, 0);
        assert!(s.aae_s.is_infinite());
        assert!(!aggregate(std::slice::from_ref(&s)).passes_gates());
        assert!(!aggregate(&[]).passes_gates(), "no songs is no pass");
        // And it survives the report's JSON: infinity goes out as
        // null and comes back infinite, not as a parse error and not
        // as zero (which would PASS the AAE gate).
        let report = Report {
            schema: REPORT_SCHEMA.to_owned(),
            aligner: "test".to_owned(),
            gated: true,
            songs: vec![s.clone()],
            by_language: by_language(std::slice::from_ref(&s)),
            all: aggregate(&[s]),
        };
        let json = serde_json::to_string(&report).expect("serialises");
        let back: Report = serde_json::from_str(&json).expect("parses");
        assert!(back.all.aae_s.is_infinite());
        assert!(!back.all.passes_gates());
    }

    #[test]
    fn the_aggregate_is_the_mean_of_songs_and_the_gates_are_the_plans() {
        let good = SongScore {
            song: "g".into(),
            language: "German".into(),
            words_truth: 100,
            words_matched: 100,
            coverage: 1.0,
            aae_s: 0.10,
            pco_01: 0.9,
            pco_03: 0.95,
            estimated_rate: 0.05,
        };
        let bad = SongScore {
            song: "b".into(),
            language: "English".into(),
            words_truth: 300,
            words_matched: 300,
            aae_s: 0.50,
            pco_01: 0.2,
            pco_03: 0.5,
            ..good.clone()
        };
        let all = aggregate(&[good.clone(), bad.clone()]);
        assert_eq!(all.songs, 2);
        // The mean of songs (0.30), not the mean over words (0.40):
        // a long song does not drown a short one.
        assert!((all.aae_s - 0.30).abs() < 1e-9, "{}", all.aae_s);
        assert!(!all.passes_gates(), "AAE must be BELOW 0.30");
        // Each gate on its own, at its boundary: exactly the gate
        // value is NOT inside it.
        let at_gate = |aae_s, pco_01, pco_03| Aggregate {
            songs: 1,
            aae_s,
            pco_01,
            pco_03,
            coverage: 1.0,
            estimated_rate: 0.0,
        };
        assert!(at_gate(0.29, 0.9, 0.9).passes_gates());
        assert!(
            !at_gate(GATE_AAE_S, 0.9, 0.9).passes_gates(),
            "AAE at the gate"
        );
        assert!(
            !at_gate(0.1, GATE_PCO_01, 0.9).passes_gates(),
            "PCO@0.1 at the gate"
        );
        assert!(
            !at_gate(0.1, 0.9, GATE_PCO_03).passes_gates(),
            "PCO@0.3 at the gate"
        );
        assert!(aggregate(std::slice::from_ref(&good)).passes_gates());
        let langs = by_language(&[good, bad]);
        assert_eq!(langs.len(), 2);
        assert_eq!(langs[0].0, "German");
        assert!((langs[0].1.pco_01 - 0.9).abs() < 1e-9);
    }

    #[test]
    fn the_corpus_layout_is_read_as_published() {
        let words = "Now\nand\nthen\n";
        let csv =
            "word_start,word_end,line_end\n32.44,32.76,nan\n32.76,32.97,nan\n32.97,33.16,33.16\n";
        let truth = parse_word_annotations(words, csv).expect("parses");
        assert_eq!(truth.len(), 3);
        assert_eq!(truth[1].text, "and");
        assert!((truth[1].start - 32.76).abs() < 1e-9);
        // A count mismatch is an error, never a silent misalignment
        // of words and times.
        assert!(parse_word_annotations("a\nb\n", csv).is_err());
        assert!(parse_word_annotations(words, "word_start,word_end,line_end\n1,2,nan\n").is_err());
        // A bad time is an error too.
        assert!(parse_word_annotations("a\n", "word_start,word_end,line_end\nx,2,nan\n").is_err());
        // Quoted commas survive the metadata split.
        assert_eq!(
            csv_split(r#"u,f.mp3,"Artist, The","A ""B"" title",Pop,BY,English"#),
            vec![
                "u",
                "f.mp3",
                "Artist, The",
                "A \"B\" title",
                "Pop",
                "BY",
                "English"
            ]
        );
    }

    #[test]
    fn a_corpus_on_disk_lists_songs_and_names_what_it_skips() {
        let dir = std::env::temp_dir().join(format!("bb-jamendo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["mp3", "lyrics", "annotations/words"] {
            std::fs::create_dir_all(dir.join(sub)).expect("dirs");
        }
        std::fs::write(
            dir.join("JamendoLyrics.csv"),
            "URL,Filepath,Artist,Title,Genre,LicenseType,Language,LyricOverlap,Polyphonic,NonLexical\n\
             u,One_-_Song.mp3,One,Song,Pop,BY,German,false,false,false\n\
             u,Two_-_Gone.mp3,Two,Gone,Pop,BY,French,false,false,false\n",
        )
        .expect("meta");
        std::fs::write(dir.join("mp3/One_-_Song.mp3"), b"not audio").expect("mp3");
        std::fs::write(dir.join("lyrics/One_-_Song.txt"), "hello world\n").expect("txt");
        std::fs::write(dir.join("lyrics/One_-_Song.words.txt"), "hello\nworld\n").expect("words");
        std::fs::write(
            dir.join("annotations/words/One_-_Song.csv"),
            "word_start,word_end,line_end\n1.0,1.5,nan\n1.6,2.0,2.0\n",
        )
        .expect("csv");
        let corpus = JamendoCorpus::load(&dir).expect("loads");
        assert_eq!(corpus.songs.len(), 1);
        assert_eq!(corpus.songs[0].language, "German");
        assert_eq!(corpus.songs[0].words.len(), 2);
        assert_eq!(corpus.skipped.len(), 1);
        assert!(
            corpus.skipped[0].1.contains("audio"),
            "{:?}",
            corpus.skipped
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
