//! A Bridge download in, a playable BG twin out (ADR-0022).
//!
//! A download is a folder with `notes.chart` or `notes.mid`, a
//! `song.ini` and one or more audio stems. [`import`] turns it into a
//! twin folder in the library:
//!
//! 1. **Which song.** A song already in the library with the same
//!    artist and title — compared by letters and digits only, so
//!    `DRAGONFORCE` is `Dragonforce` and `What's` is `Whats` — is the
//!    original, and the twin takes ITS spelling of both, so the
//!    browser files it under that song. Without one the download
//!    becomes a song of its own (only BG versions).
//! 2. **Which number.** The fingerprint of the chart and `song.ini`
//!    is compared with every BG version of that song: the same
//!    download is found again and nothing is written; another one
//!    gets the lowest free number, `BG-01` … `BG-99`.
//! 3. **The audio** is mixed from the stems into one `song.m4a` by the
//!    transcoder the caller passes (ffmpeg, [`ffmpeg_transcoder`]) —
//!    BeatByte plays one file and cannot decode Opus. It is then
//!    decoded once: a file that does not play fails the import here,
//!    not in the middle of a song.
//! 4. **The chart** is converted ([`beatbyte_chart::bridge`]),
//!    validated like any other, and written LAST, beside
//!    [`SOURCE_FILE`] — so a scan never sees a twin without its audio,
//!    and a run that fails partway leaves a folder no one plays.
//!
//! No clock: `now_ms` is passed in. The only process it starts is the
//! transcoder it is given.

use std::path::{Path, PathBuf};

use beatbyte_chart::bridge::{self as convert, BridgeError, Report};
use beatbyte_chart::twin;
use beatbyte_chart::versions;
use serde::{Deserialize, Serialize};

/// The sidecar that says where a BG twin came from.
pub const SOURCE_FILE: &str = "bridge-source.json";

/// The audio file a BG twin plays.
pub const AUDIO_FILE: &str = "song.m4a";

/// Audio a download may carry, by extension.
const AUDIO_EXTENSIONS: [&str; 6] = ["opus", "ogg", "mp3", "wav", "flac", "m4a"];

/// Stems that are not part of the song (the download's preview clip).
const NOT_A_STEM: [&str; 1] = ["preview"];

/// Which chart format a download carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// `notes.chart`.
    Chart,
    /// `notes.mid`.
    Mid,
}

/// A Bridge download, as found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// The folder.
    pub folder: PathBuf,
    /// Its chart file.
    pub chart: PathBuf,
    /// Which format that is.
    pub format: Format,
    /// Its `song.ini`, when there is one.
    pub ini: Option<PathBuf>,
    /// Its audio stems, sorted by name.
    pub stems: Vec<PathBuf>,
}

/// What [`SOURCE_FILE`] holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    /// The download folder it was converted from.
    pub source: String,
    /// [`fingerprint`] of that download.
    pub fingerprint: String,
    /// The chart's format.
    pub format: Format,
    /// Who charted it, when `song.ini` says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<String>,
    /// When it was converted, Unix milliseconds.
    pub imported_ms: u64,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A new BG version was written.
    Imported {
        /// Its folder.
        folder: PathBuf,
        /// Its number.
        number: u8,
        /// The library song it sits under, by folder name, if any.
        original: Option<String>,
        /// What the conversion changed or left out.
        report: Report,
    },
    /// This download is already BG version `number` in `folder`.
    AlreadyThere {
        /// The existing folder.
        folder: PathBuf,
        /// Its number.
        number: u8,
    },
}

/// Mixes stems into one AAC file at the given path.
pub type Transcoder<'a> = dyn Fn(&[PathBuf], &Path) -> Result<(), String> + 'a;

/// The download in `folder`, if it is one: a chart file and at least
/// one audio stem. `notes.chart` wins over `notes.mid` when both are
/// there (it is what the format's own player reads first).
///
/// # Errors
/// When the folder cannot be read or is not a download.
pub fn inspect(folder: &Path) -> Result<Download, String> {
    let names = twin::names_in(folder)?;
    let has = |name: &str| names.iter().find(|n| n.eq_ignore_ascii_case(name));
    let (chart, format) = match (has("notes.chart"), has("notes.mid")) {
        (Some(name), _) => (folder.join(name), Format::Chart),
        (None, Some(name)) => (folder.join(name), Format::Mid),
        (None, None) => {
            return Err(format!(
                "{} has no notes.chart or notes.mid",
                folder.display()
            ));
        }
    };
    let ini = has("song.ini").map(|n| folder.join(n));
    let mut stems: Vec<PathBuf> = names
        .iter()
        .filter(|name| is_stem(name))
        .map(|name| folder.join(name))
        .collect();
    stems.sort();
    if stems.is_empty() {
        return Err(format!("{} has no audio", folder.display()));
    }
    Ok(Download {
        folder: folder.to_path_buf(),
        chart,
        format,
        ini,
        stems,
    })
}

/// Whether a file name is an audio stem of the song. Pure — tested.
#[must_use]
pub fn is_stem(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !name.starts_with('.')
        && AUDIO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        && !NOT_A_STEM.contains(&stem.to_ascii_lowercase().as_str())
}

/// Every download under `root`, its own folder included, at most three
/// levels down, sorted. Unreadable folders are skipped.
#[must_use]
pub fn downloads_under(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if inspect(dir).is_ok() {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for sub in dirs {
            walk(&sub, depth - 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, 3, &mut out);
    out
}

/// A download's identity: FNV-1a over its chart file and `song.ini`.
/// Two downloads with the same bytes are the same chart, wherever
/// they sit; the audio is not part of it (Bridge can fetch the same
/// chart with or without a video, the chart is what is played).
///
/// # Errors
/// When the chart cannot be read.
pub fn fingerprint(download: &Download) -> Result<String, String> {
    let read = |path: &Path| {
        std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let mut hash = crate::folder::fnv1a_update(0xcbf2_9ce4_8422_2325, &read(&download.chart)?);
    // A separator, so moving bytes between the files moves the hash.
    hash = crate::folder::fnv1a_update(hash, &[0xff]);
    if let Some(ini) = &download.ini {
        hash = crate::folder::fnv1a_update(hash, &read(ini)?);
    }
    Ok(format!("{hash:016x}"))
}

/// A title or artist reduced to lowercase letters and digits — what two
/// spellings of one song have in common. Pure — tested.
#[must_use]
pub fn match_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A song already in the library, as far as matching needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySong {
    /// Its folder name.
    pub folder: String,
    /// Its title as its chart spells it.
    pub title: String,
    /// Its artist as its chart spells it.
    pub artist: String,
}

/// The songs in `library_root` that a BG twin could sit under: every
/// folder that is not itself a twin and whose base chart names a
/// title. Unreadable folders are skipped.
#[must_use]
pub fn library_songs(library_root: &Path) -> Vec<LibrarySong> {
    #[derive(Deserialize)]
    struct Head {
        song: HeadSong,
    }
    #[derive(Deserialize)]
    struct HeadSong {
        title: String,
        #[serde(default)]
        artist: String,
    }
    let Ok(entries) = std::fs::read_dir(library_root) else {
        return Vec::new();
    };
    let mut out: Vec<LibrarySong> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let folder = entry.file_name().to_string_lossy().into_owned();
            if twin::is_twin_folder(&folder) {
                return None;
            }
            let text = std::fs::read_to_string(entry.path().join(versions::BASE_CHART)).ok()?;
            let head: Head = serde_json::from_str(&text).ok()?;
            Some(LibrarySong {
                folder,
                title: head.song.title,
                artist: head.song.artist,
            })
        })
        .collect();
    out.sort_by(|a, b| a.folder.cmp(&b.folder));
    out
}

/// The library song a download of `artist` – `title` belongs to:
/// artist and title equal by [`match_key`]. Several matches (the same
/// song imported twice) resolve to the first by folder name, so the
/// choice never depends on directory order. Pure — tested.
#[must_use]
pub fn find_original<'a>(
    songs: &'a [LibrarySong],
    artist: &str,
    title: &str,
) -> Option<&'a LibrarySong> {
    let (artist, title) = (match_key(artist), match_key(title));
    if title.is_empty() {
        return None;
    }
    songs
        .iter()
        .find(|song| match_key(&song.artist) == artist && match_key(&song.title) == title)
}

/// A folder name for a song that has no folder yet. Pure — tested.
#[must_use]
pub fn standalone_base(artist: &str, title: &str) -> String {
    let cleaned: String = format!("{artist} - {title}")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "bridge-song".to_owned()
    } else {
        trimmed.chars().take(60).collect()
    }
}

/// Every BG version of the song whose folder name is `base`, as
/// `(number, folder, its source record if readable)`, by number.
fn versions_of(library_root: &Path, base: &str) -> Vec<(u8, PathBuf, Option<SourceRecord>)> {
    let Ok(entries) = std::fs::read_dir(library_root) else {
        return Vec::new();
    };
    let mut out: Vec<(u8, PathBuf, Option<SourceRecord>)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = twin::bridge_number_of_folder(&name)?;
            (twin::bridge_folder_name(base, number) == name).then(|| {
                let record = std::fs::read_to_string(entry.path().join(SOURCE_FILE))
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok());
                (number, entry.path(), record)
            })
        })
        .collect();
    out.sort_by_key(|v| v.0);
    out
}

/// Convert the download in `source` into a BG twin under
/// `library_root` (the folder that holds the song folders).
///
/// # Errors
/// When the download cannot be read or converted, the audio cannot be
/// made or does not play, all 99 numbers are taken, or the twin cannot
/// be written. A failed import leaves no chart behind.
pub fn import(
    source: &Path,
    library_root: &Path,
    transcode: &Transcoder<'_>,
    now_ms: u64,
) -> Result<Outcome, String> {
    let download = inspect(source)?;
    let fingerprint = fingerprint(&download)?;
    let ini = match &download.ini {
        Some(path) => convert::parse_ini(
            &std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?,
        ),
        None => convert::SongIni::default(),
    };
    let song = match download.format {
        Format::Chart => convert::text::parse_chart(
            &std::fs::read_to_string(&download.chart)
                .map_err(|e| format!("cannot read {}: {e}", download.chart.display()))?,
        ),
        Format::Mid => convert::midi::parse_midi(
            &std::fs::read(&download.chart)
                .map_err(|e| format!("cannot read {}: {e}", download.chart.display()))?,
        ),
    }
    .map_err(|e: BridgeError| e.to_string())?;
    let (mut chart, report) =
        convert::assemble(&song, &ini, AUDIO_FILE).map_err(|e| e.to_string())?;

    // Which song, and its spelling.
    let songs = library_songs(library_root);
    let original = find_original(&songs, &chart.song.artist, &chart.song.title).cloned();
    let base = match &original {
        Some(song) => {
            chart.song.title.clone_from(&song.title);
            chart.song.artist.clone_from(&song.artist);
            song.folder.clone()
        }
        None => standalone_base(&chart.song.artist, &chart.song.title),
    };

    // Which number — or nothing to do.
    let existing = versions_of(library_root, &base);
    if let Some((number, folder, _)) = existing.iter().find(|(_, folder, record)| {
        twin::is_finished(folder)
            && record
                .as_ref()
                .is_some_and(|r| r.fingerprint == fingerprint)
    }) {
        return Ok(Outcome::AlreadyThere {
            folder: folder.clone(),
            number: *number,
        });
    }
    let taken: Vec<u8> = existing
        .iter()
        .filter(|(_, folder, _)| twin::is_finished(folder))
        .map(|v| v.0)
        .collect();
    let number = twin::next_bridge_number(&taken)
        .ok_or_else(|| format!("{base} already has {} BG versions", twin::MAX_BRIDGE_NUMBER))?;
    chart.song.title = twin::bridge_title(&chart.song.title, number);
    let folder = library_root.join(twin::bridge_folder_name(&base, number));

    // An unfinished folder of that name is a failed earlier run: it
    // is ours to replace.
    if folder.exists() {
        std::fs::remove_dir_all(&folder)
            .map_err(|e| format!("cannot clear {}: {e}", folder.display()))?;
    }
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let record = SourceRecord {
        source: download.folder.display().to_string(),
        fingerprint,
        format: download.format,
        charter: ini.charter.clone(),
        imported_ms: now_ms,
    };
    let result = write_twin(&download, &folder, &mut chart, transcode, &record);
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(error);
    }
    Ok(Outcome::Imported {
        folder,
        number,
        original: original.map(|o| o.folder),
        report,
    })
}

fn write_twin(
    download: &Download,
    folder: &Path,
    chart: &mut beatbyte_chart::ChartFile,
    transcode: &Transcoder<'_>,
    record: &SourceRecord,
) -> Result<(), String> {
    let audio = folder.join(AUDIO_FILE);
    transcode(&download.stems, &audio)?;
    // It must play, and its timeline must be the one the chart is on.
    let decoded = beatbyte_audio::decode_file(&audio)
        .map_err(|e| format!("the converted audio does not play: {e}"))?;
    let priming = decoded.priming();
    chart.audio_trim = Some(beatbyte_chart::AudioTrim::declared(
        priming.samples,
        priming.timescale,
        decoded.sample_rate(),
    ));
    if chart.song.duration_s.is_none() {
        chart.song.duration_s = Some(decoded.duration_s());
    }
    let errors: Vec<String> = chart
        .validate()
        .into_iter()
        .filter(|i| i.severity == beatbyte_chart::Severity::Error)
        .map(|i| format!("{}: {}", i.location, i.message))
        .collect();
    if !errors.is_empty() {
        return Err(format!(
            "the converted chart is invalid: {}",
            errors.join("; ")
        ));
    }
    let text = serde_json::to_string_pretty(record)
        .map_err(|e| format!("cannot serialize the source record: {e}"))?;
    beatbyte_chart::io::write_atomic(&folder.join(SOURCE_FILE), text.as_bytes())
        .map_err(|e| format!("cannot write {SOURCE_FILE}: {e}"))?;
    // The chart last: until it exists, the folder is not a twin.
    beatbyte_chart::save_chart_file(&folder.join(versions::BASE_CHART), chart)
        .map_err(|e| format!("cannot write the chart: {e}"))
}

/// Where Bridge keeps its settings, under the platform's config
/// directory (`~/Library/Application Support` on macOS).
#[must_use]
pub fn bridge_settings_path(config_dir: &Path) -> PathBuf {
    config_dir
        .join("Bridge")
        .join("bridge_data")
        .join("settings.json")
}

/// The download folder a Bridge `settings.json` names
/// (`libraryPath`), if it names one. Pure — tested.
#[must_use]
pub fn library_path_from_settings(text: &str) -> Option<PathBuf> {
    #[derive(Deserialize)]
    struct Settings {
        #[serde(rename = "libraryPath")]
        library_path: Option<String>,
    }
    serde_json::from_str::<Settings>(text)
        .ok()?
        .library_path
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
}

/// Where ffmpeg is: `PATH`, then the two places Homebrew puts it — a
/// game started from the Finder does not inherit the shell's `PATH`.
#[must_use]
pub fn find_ffmpeg() -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH").into_iter().flat_map(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("ffmpeg"))
            .collect::<Vec<_>>()
    });
    from_path
        .chain(
            [
                "/opt/homebrew/bin/ffmpeg",
                "/usr/local/bin/ffmpeg",
                "/usr/bin/ffmpeg",
            ]
            .into_iter()
            .map(PathBuf::from),
        )
        .find(|candidate| candidate.is_file())
}

/// The ffmpeg command line that mixes `stems` into `out`: summed (not
/// averaged — a stem split is a mix taken apart, adding the parts puts
/// it back), resampled to 44.1 kHz, AAC at 256 kbit/s. Pure — tested.
#[must_use]
pub fn ffmpeg_args(stems: &[PathBuf], out: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
    ];
    for stem in stems {
        args.push("-i".into());
        args.push(stem.display().to_string());
    }
    if stems.len() > 1 {
        args.push("-filter_complex".into());
        args.push(format!(
            "amix=inputs={}:duration=longest:normalize=0",
            stems.len()
        ));
    }
    args.extend(
        [
            "-vn", "-ar", "44100", "-ac", "2", "-c:a", "aac", "-b:a", "256k",
        ]
        .into_iter()
        .map(String::from),
    );
    args.push(out.display().to_string());
    args
}

/// A transcoder that runs `ffmpeg`.
pub fn ffmpeg_transcoder(ffmpeg: PathBuf) -> impl Fn(&[PathBuf], &Path) -> Result<(), String> {
    move |stems, out| {
        let output = std::process::Command::new(&ffmpeg)
            .args(ffmpeg_args(stems, out))
            .output()
            .map_err(|e| format!("cannot run {}: {e}", ffmpeg.display()))?;
        if output.status.success() && out.is_file() {
            Ok(())
        } else {
            Err(format!(
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "beatbyte-bridge-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const CHART: &str = "[Song]\n{\n  Resolution = 192\n}\n[SyncTrack]\n{\n  0 = B 120000\n}\n\
        [ExpertSingle]\n{\n  192 = N 0 0\n  384 = N 1 0\n  576 = N 2 0\n}\n";

    fn download(dir: &Path, artist: &str, title: &str, charter: &str) -> PathBuf {
        let folder = dir.join(format!("{artist} - {title} ({charter})"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("notes.chart"), CHART).unwrap();
        std::fs::write(
            folder.join("song.ini"),
            format!("[song]\nname = {title}\nartist = {artist}\ncharter = {charter}\n"),
        )
        .unwrap();
        std::fs::write(folder.join("song.opus"), b"not really opus").unwrap();
        std::fs::write(folder.join("preview.ogg"), b"clip").unwrap();
        folder
    }

    /// Stands in for ffmpeg: copies a real, tiny m4a into place.
    fn fake_transcoder(stems: &[PathBuf], out: &Path) -> Result<(), String> {
        assert!(stems.iter().all(|s| !s.ends_with("preview.ogg")));
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../beatbyte-audio/tests/fixtures/click-ffmpeg.m4a");
        std::fs::copy(fixture, out)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn library_song(root: &Path, folder: &str, artist: &str, title: &str) {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("chart.json"),
            format!(
                r#"{{"format_version":1,"song":{{"title":"{title}","artist":"{artist}","audio":"a.m4a","bpm":120.0}},"charts":[]}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn stems_are_audio_files_and_the_preview_is_not_one() {
        assert!(is_stem("song.opus"));
        assert!(is_stem("Guitar.OGG"));
        assert!(!is_stem("preview.ogg"));
        assert!(!is_stem("album.jpg"));
        assert!(!is_stem(".song.opus"));
        assert!(!is_stem("notes.mid"));
    }

    #[test]
    fn spellings_match_by_letters_and_digits_only() {
        assert_eq!(match_key("DRAGONFORCE"), match_key("DragonForce"));
        assert_eq!(match_key("What's Love"), "whatslove");
        let songs = vec![
            LibrarySong {
                folder: "b".into(),
                title: "Through the Fire and Flames".into(),
                artist: "DRAGONFORCE".into(),
            },
            LibrarySong {
                folder: "a".into(),
                title: "Through the Fire and Flames".into(),
                artist: "Someone Else".into(),
            },
        ];
        assert_eq!(
            find_original(&songs, "Dragonforce", "Through The Fire and Flames")
                .map(|s| &s.folder[..]),
            Some("b")
        );
        assert_eq!(
            find_original(&songs, "Dragonforce", "Fury of the Storm"),
            None
        );
    }

    #[test]
    fn a_download_is_found_with_its_chart_ini_and_stems() {
        let dir = Scratch::new("inspect");
        let folder = download(&dir.0, "Band", "Song", "Me");
        std::fs::write(folder.join("guitar.opus"), b"x").unwrap();
        let found = inspect(&folder).unwrap();
        assert_eq!(found.format, Format::Chart);
        assert!(found.ini.is_some());
        assert_eq!(found.stems.len(), 2);
        assert_eq!(downloads_under(&dir.0), vec![folder]);
        assert!(inspect(&dir.0).is_err());
    }

    #[test]
    fn a_download_of_a_library_song_becomes_bg_01_under_it_in_its_spelling() {
        let dir = Scratch::new("under");
        let library = dir.0.join("imported");
        library_song(
            &library,
            "dragonforce---ttfaf-m4a",
            "DRAGONFORCE",
            "Through the Fire and Flames",
        );
        let source = download(
            &dir.0,
            "DragonForce",
            "Through The Fire and Flames",
            "Stargazer",
        );
        let outcome = import(&source, &library, &fake_transcoder, 7).unwrap();
        let Outcome::Imported {
            folder,
            number,
            original,
            report,
        } = outcome
        else {
            panic!("expected an import");
        };
        assert_eq!(number, 1);
        assert_eq!(original.as_deref(), Some("dragonforce---ttfaf-m4a"));
        assert_eq!(folder, library.join("bridge-01-dragonforce---ttfaf-m4a"));
        assert_eq!(report.notes.len(), 1);
        let chart = beatbyte_chart::load_chart_file(&folder.join("chart.json")).unwrap();
        assert_eq!(chart.song.title, "[BG-01] Through the Fire and Flames");
        assert_eq!(chart.song.artist, "DRAGONFORCE");
        assert_eq!(chart.song.audio, AUDIO_FILE);
        assert!(chart.audio_trim.is_some());
        assert!(chart.grid.is_some());
        assert_eq!(
            twin::base_title(&chart.song.title),
            Some("Through the Fire and Flames"),
            "the browser must find the original by the twin's title"
        );
        let record: SourceRecord =
            serde_json::from_str(&std::fs::read_to_string(folder.join(SOURCE_FILE)).unwrap())
                .unwrap();
        assert_eq!(record.charter.as_deref(), Some("Stargazer"));
        assert_eq!(record.imported_ms, 7);
    }

    #[test]
    fn the_same_download_twice_writes_nothing_and_another_gets_the_next_number() {
        let dir = Scratch::new("numbers");
        let library = dir.0.join("imported");
        std::fs::create_dir_all(&library).unwrap();
        let first = download(&dir.0, "Band", "Song", "One");
        let second = download(&dir.0, "Band", "Song", "Two");
        let outcome = |source: &Path| import(source, &library, &fake_transcoder, 0).unwrap();
        assert!(matches!(
            outcome(&first),
            Outcome::Imported { number: 1, .. }
        ));
        assert!(matches!(
            outcome(&first),
            Outcome::AlreadyThere { number: 1, .. }
        ));
        assert!(matches!(
            outcome(&second),
            Outcome::Imported { number: 2, .. }
        ));
        // A song not in the library stands alone, under its own name.
        assert!(
            library
                .join("bridge-01-band---song")
                .join("chart.json")
                .is_file()
        );
        assert!(
            library
                .join("bridge-02-band---song")
                .join("chart.json")
                .is_file()
        );
        // A gap is filled first.
        std::fs::remove_dir_all(library.join("bridge-01-band---song")).unwrap();
        let third = download(&dir.0, "Band", "Song", "Three");
        assert!(matches!(
            outcome(&third),
            Outcome::Imported { number: 1, .. }
        ));
    }

    #[test]
    fn a_failed_conversion_leaves_no_folder_behind() {
        let dir = Scratch::new("fail");
        let library = dir.0.join("imported");
        std::fs::create_dir_all(&library).unwrap();
        let source = download(&dir.0, "Band", "Song", "One");
        let error = import(
            &source,
            &library,
            &|_: &[PathBuf], _: &Path| Err("no ffmpeg".into()),
            0,
        )
        .unwrap_err();
        assert!(error.contains("no ffmpeg"));
        assert_eq!(std::fs::read_dir(&library).unwrap().count(), 0);
        // And audio that does not decode fails the import too.
        let error = import(
            &source,
            &library,
            &|_: &[PathBuf], out: &Path| std::fs::write(out, b"garbage").map_err(|e| e.to_string()),
            0,
        )
        .unwrap_err();
        assert!(error.contains("does not play"), "{error}");
        assert_eq!(std::fs::read_dir(&library).unwrap().count(), 0);
    }

    #[test]
    fn ffmpeg_sums_several_stems_and_transcodes_one() {
        let one = ffmpeg_args(&[PathBuf::from("a.opus")], Path::new("o.m4a"));
        assert!(!one.iter().any(|a| a.contains("amix")));
        assert!(one.windows(2).any(|w| w == ["-c:a", "aac"]));
        let two = ffmpeg_args(
            &[PathBuf::from("a.opus"), PathBuf::from("b.opus")],
            Path::new("o.m4a"),
        );
        assert!(
            two.iter()
                .any(|a| a == "amix=inputs=2:duration=longest:normalize=0")
        );
        assert_eq!(two.last().map(String::as_str), Some("o.m4a"));
    }

    #[test]
    fn bridges_download_folder_is_read_from_its_settings() {
        assert_eq!(
            library_path_from_settings(r#"{"theme":"dark","libraryPath":"/Volumes/X/charts"}"#),
            Some(PathBuf::from("/Volumes/X/charts"))
        );
        assert_eq!(library_path_from_settings(r#"{"libraryPath":""}"#), None);
        assert_eq!(library_path_from_settings("not json"), None);
    }

    #[test]
    fn standalone_folder_names_are_filesystem_safe() {
        assert_eq!(
            standalone_base("Tina Turner", "What's Love"),
            "tina-turner---what-s-love"
        );
        assert_eq!(standalone_base("", "///"), "bridge-song");
    }
}
