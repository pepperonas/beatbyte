//! `beatbyte-cli loudness …` — measure a song (or a library) the way
//! the game levels it, print the table, and with `--write` put the
//! sidecar beside each audio file so the game applies the gain.
//!
//! IO and printing here; the meter, the gain law and the verdict are
//! `beatbyte_audio::loudness` / `quality`, pure and tested.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_audio::loudness::{self, CEILING_DBTP, Report, TARGET_LUFS};
use beatbyte_audio::quality::Verdict;
use beatbyte_chart::{ChartFile, versions};

/// The build's name in a sidecar.
fn measured_by() -> String {
    format!("beatbyte {}", env!("CARGO_PKG_VERSION"))
}

/// The audio file a chart path or song folder points at.
fn audio_of(path: &Path) -> Result<PathBuf, String> {
    let (folder, chart_path) = if path.is_dir() {
        let names: Vec<String> = std::fs::read_dir(path)
            .map_err(|error| format!("cannot list `{}`: {error}", path.display()))?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        if !names.iter().any(|n| n == versions::BASE_CHART) {
            return Err(format!(
                "no `{}` — legacy layout, skipped",
                versions::BASE_CHART
            ));
        }
        let pointer = std::fs::read_to_string(path.join(versions::POINTER_FILE)).ok();
        let active = versions::resolve_active(pointer.as_deref(), &names);
        (path.to_path_buf(), path.join(active))
    } else if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        let folder = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        (folder, path.to_path_buf())
    } else {
        // An audio file straight.
        return Ok(path.to_path_buf());
    };
    let text = std::fs::read_to_string(&chart_path)
        .map_err(|error| format!("cannot read `{}`: {error}", chart_path.display()))?;
    let chart = ChartFile::from_json(&text).map_err(|error| format!("{error}"))?;
    beatbyte_chart::resolve_audio_path(&folder, &chart.song.audio).map_err(|e| e.to_string())
}

/// One measured song, for the table.
struct Row {
    name: String,
    report: Report,
}

fn measure_one(path: &Path, write: bool) -> Result<Row, String> {
    let audio = audio_of(path)?;
    let report = loudness::measure_file(&audio, &measured_by()).map_err(|e| e.to_string())?;
    if write {
        loudness::write_report(&audio, &report).map_err(|error| {
            format!(
                "cannot write `{}`: {error}",
                loudness::sidecar_path(&audio).display()
            )
        })?;
    }
    let name = audio
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Row { name, report })
}

fn print_row(row: &Row) {
    let m = &row.report.measurement;
    let q = &row.report.quality;
    let lufs = m
        .integrated_lufs
        .map_or("   n/a".to_owned(), |l| format!("{l:6.1}"));
    let limited = if row.report.peak_limited() {
        " peak-limited"
    } else {
        ""
    };
    let issues: Vec<&str> = q.issues.iter().map(|i| i.what.as_str()).collect();
    println!(
        "| {:<34} | {lufs} | {:5.1} | {:6.1} | {:+5.1}{limited} | {:5} | {} |",
        row.name.chars().take(34).collect::<String>(),
        m.loudness_range_lu.unwrap_or(0.0),
        m.true_peak_dbtp,
        row.report.gain_db(),
        q.verdict.label(),
        if issues.is_empty() {
            "–".to_owned()
        } else {
            issues.join("; ")
        }
    );
}

fn header() {
    println!(
        "| Song | LUFS | LRA | dBTP | gain dB | quality | notes |\n|---|---:|---:|---:|---:|---|---|"
    );
}

/// `loudness <path>`: one song (a chart, a song folder or an audio file).
pub fn run(path: &Path, write: bool) -> ExitCode {
    header();
    match measure_one(path, write) {
        Ok(row) => {
            print_row(&row);
            eprintln!(
                "target {TARGET_LUFS:.0} LUFS, ceiling {CEILING_DBTP:.0} dBTP{}",
                if write { "; sidecar written" } else { "" }
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

/// `loudness <dir> --all`: every song folder under `dir`, with the
/// spread before and after.
pub fn run_all(dir: &Path, write: bool) -> ExitCode {
    let mut folders: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(error) => {
            eprintln!("cannot list `{}`: {error}", dir.display());
            return ExitCode::from(2);
        }
    };
    folders.sort();
    header();
    let mut rows = Vec::new();
    let mut failed = 0usize;
    for folder in &folders {
        match measure_one(folder, write) {
            Ok(row) => {
                print_row(&row);
                rows.push(row);
            }
            Err(error) => {
                eprintln!("{}: {error}", folder.display());
                failed += 1;
            }
        }
    }
    summarise(&rows, failed, write);
    if rows.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// The library in numbers: the loudness spread as the files are and
/// as the game will play them. Pure over the rows — printed once.
fn summarise(rows: &[Row], failed: usize, write: bool) {
    let before: Vec<f64> = rows
        .iter()
        .filter_map(|r| r.report.measurement.integrated_lufs)
        .collect();
    if before.is_empty() {
        eprintln!("{failed} failed, nothing measured");
        return;
    }
    let after: Vec<f64> = rows
        .iter()
        .filter_map(|r| {
            r.report
                .measurement
                .integrated_lufs
                .map(|l| l + r.report.gain_db())
        })
        .collect();
    let stats = |v: &[f64]| {
        let n = v.len() as f64;
        let mean = v.iter().sum::<f64>() / n;
        let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
        let min = v.iter().copied().fold(f64::INFINITY, f64::min);
        let max = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (mean, sd, min, max)
    };
    let (bm, bs, bmin, bmax) = stats(&before);
    let (am, as_, amin, amax) = stats(&after);
    let limited = rows.iter().filter(|r| r.report.peak_limited()).count();
    let count = |v: Verdict| {
        rows.iter()
            .filter(|r| r.report.quality.verdict == v)
            .count()
    };
    eprintln!(
        "{} songs measured, {failed} failed{}",
        rows.len(),
        if write { ", sidecars written" } else { "" }
    );
    eprintln!(
        "as the files are: {bm:.1} LUFS mean, sd {bs:.1}, {bmin:.1} to {bmax:.1} ({:.1} dB spread)",
        bmax - bmin
    );
    eprintln!(
        "as the game plays them: {am:.1} LUFS mean, sd {as_:.1}, {amin:.1} to {amax:.1} ({:.1} dB spread); {limited} peak-limited",
        amax - amin
    );
    eprintln!(
        "quality: {} good, {} fair, {} poor",
        count(Verdict::Good),
        count(Verdict::Fair),
        count(Verdict::Poor)
    );
}
