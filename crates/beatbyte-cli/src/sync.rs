//! `beatbyte-cli sync` — one career on several devices (ADR-0021).
//!
//! The hub is a directory any device can reach with rsync (the raspi5
//! over SSH, or a plain path): each device owns `devices/<id>/` and
//! publishes its WHOLE merged state there; song files travel as
//! content-addressed blobs under `blobs/<sha256>`. A sync is:
//!
//! 1. take the hub's lock (`mkdir lock` — atomic, on any filesystem);
//! 2. pull every other device's folder into `<data>/sync/remote/`;
//! 3. merge each of them into the local data, one rule per kind of
//!    data (`beatbyte-sync` decides, this module only carries out);
//! 4. publish: the blobs the hub lacks, the telemetry copy, the
//!    library manifest, and the snapshot LAST — a device reading the
//!    hub between two of these steps still sees the previous
//!    snapshot, which names only blobs that are there;
//! 5. release the lock.
//!
//! It refuses while the game runs: the game rewrites its files on
//! exit and would undo the merge (and a live telemetry store is not
//! ours to write).
//!
//! ⚠️ The API key never travels: the snapshot carries the SHARED
//! settings only (`beatbyte_sync::settings::shared_part`).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use beatbyte_core::player::Roster;
use beatbyte_sync::library::{Action, Entry, Manifest, POINTER, version_of};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The snapshot format this build writes and reads.
const FORMAT: u64 = 1;

/// A lock older than this is a crashed sync, and is broken.
const STALE_LOCK_MS: u64 = 30 * 60 * 1000;

/// The hub a device syncs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hub {
    /// A directory on this machine (tests, a mounted share).
    Local(PathBuf),
    /// `host:path` over SSH.
    Ssh {
        /// The SSH destination (`raspi5`, `pi@192.168.178.105`).
        host: String,
        /// The hub directory on that host.
        path: String,
    },
}

impl Hub {
    /// `host:path` is SSH, anything else a local directory.
    #[must_use]
    pub fn parse(spec: &str) -> Hub {
        match spec.split_once(':') {
            Some((host, path)) if !host.is_empty() && !host.contains('/') => Hub::Ssh {
                host: host.to_owned(),
                path: path.to_owned(),
            },
            _ => Hub::Local(PathBuf::from(spec)),
        }
    }

    fn spec(&self) -> String {
        match self {
            Hub::Local(path) => path.to_string_lossy().into_owned(),
            Hub::Ssh { host, path } => format!("{host}:{path}"),
        }
    }

    /// An rsync operand for `rel` under the hub.
    fn rsync_path(&self, rel: &str) -> String {
        match self {
            Hub::Local(root) => root.join(rel).to_string_lossy().into_owned(),
            Hub::Ssh { host, path } => format!("{host}:{path}/{rel}"),
        }
    }

    /// Run a shell command in the hub directory; its stdout.
    fn shell(&self, script: &str) -> Result<String, String> {
        let output = match self {
            Hub::Local(root) => {
                std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
                Command::new("sh")
                    .arg("-c")
                    .arg(script)
                    .current_dir(root)
                    .output()
            }
            Hub::Ssh { host, path } => Command::new("ssh")
                .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host])
                .arg(format!(
                    "mkdir -p {q} && cd {q} && {script}",
                    q = quote(path)
                ))
                .output(),
        }
        .map_err(|e| format!("hub {}: {e}", self.spec()))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!(
                "hub {}: {}",
                self.spec(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

/// Single-quote for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn rsync(args: &[&str]) -> Result<(), String> {
    let output = Command::new("rsync")
        .args(["-rt", "-e", "ssh -o BatchMode=yes -o ConnectTimeout=10"])
        .args(args)
        .output()
        .map_err(|e| format!("rsync: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "rsync {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// What `run` is told.
#[derive(Debug, Clone)]
pub struct Options {
    /// The data directory (`<data>/beatbyte`).
    pub data: PathBuf,
    /// The settings file — the game keeps it in the CONFIG directory,
    /// which is the data directory on macOS but not on Linux.
    pub settings: PathBuf,
    /// The hub; `None` = the one remembered from the last sync.
    pub hub: Option<String>,
    /// Plan and report, change nothing here or on the hub.
    pub dry_run: bool,
    /// Also carry the ML models (once — existing files are skipped).
    pub models: bool,
    /// Refuse while a game process runs (always on outside tests).
    pub check_game: bool,
}

/// The data directory the game uses.
#[must_use]
pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte"))
}

/// The settings file the game uses.
#[must_use]
pub fn default_settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("beatbyte").join("settings.json"))
}

/// The command-line entry.
pub fn run(options: &Options) -> ExitCode {
    match sync(options) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("sync: {message}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------- state

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    device: String,
    #[serde(default)]
    hub: Option<String>,
}

fn sync_dir(data: &Path) -> PathBuf {
    data.join("sync")
}

fn library_root(data: &Path) -> PathBuf {
    data.join("songs").join("imported")
}

fn hostname() -> String {
    let raw = Command::new("hostname")
        .arg("-s")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if clean.is_empty() {
        "device".to_owned()
    } else {
        clean
    }
}

/// The device's config: its id is made once and never changes, so
/// renaming the machine does not orphan its folder on the hub.
fn load_config(data: &Path, dry_run: bool) -> Result<Config, String> {
    let path = sync_dir(data).join("device.json");
    if let Ok(text) = std::fs::read_to_string(&path) {
        return serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()));
    }
    let config = Config {
        device: format!("{}-{:x}", hostname(), now_ms()),
        hub: None,
    };
    if !dry_run {
        write_json(&path, &serde_json::to_value(&config).unwrap_or_default())?;
    }
    Ok(config)
}

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// Write through a temporary file and a rename: a crash leaves the old
/// file or the new one, never half of either.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp-sync");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    write_atomic(path, text.as_bytes())
}

// ---------------------------------------------------------------- counts

/// What a data directory holds — printed before and after, so a sync
/// can be checked by its numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    /// Players in the roster.
    pub players: usize,
    /// Lines in the play log.
    pub history: usize,
    /// Best-score records.
    pub scores: usize,
    /// (player, achievement) pairs.
    pub achievements: usize,
    /// Telemetry sessions.
    pub sessions: u64,
    /// Telemetry events.
    pub events: u64,
    /// Files in the library.
    pub files: usize,
    /// Bytes in the library.
    pub bytes: u64,
}

impl std::fmt::Display for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "players {} · history {} · scores {} · achievements {} · sessions {} · events {} · library {} files, {:.2} GB",
            self.players,
            self.history,
            self.scores,
            self.achievements,
            self.sessions,
            self.events,
            self.files,
            self.bytes as f64 / 1e9
        )
    }
}

/// Count what `data` holds.
#[must_use]
pub fn counts(data: &Path) -> Counts {
    let roster: Roster = read_json(&data.join("players.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let history = std::fs::read_to_string(data.join("history.jsonl")).unwrap_or_default();
    let scores = read_json(&data.join("scores.json"))
        .and_then(|v| v.get("records").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0);
    let achievements = read_json(&data.join("achievements.json"))
        .map_or(0, |v| beatbyte_sync::achievements::count(&v));
    let (sessions, events) = telemetry_counts(&data.join("telemetry.db"));
    let mut files = Vec::new();
    walk(&library_root(data), "", &mut files);
    let bytes = files
        .iter()
        .filter_map(|rel| std::fs::metadata(library_root(data).join(rel)).ok())
        .map(|m| m.len())
        .sum();
    Counts {
        players: roster.players.len(),
        history: beatbyte_sync::history::count(&history),
        scores,
        achievements,
        sessions,
        events,
        files: files.len(),
        bytes,
    }
}

fn telemetry_counts(path: &Path) -> (u64, u64) {
    if !path.exists() {
        return (0, 0);
    }
    let Ok(store) = beatbyte_telemetry::Store::open_readonly(path) else {
        return (0, 0);
    };
    beatbyte_telemetry::merge::summaries(&store).map_or((0, 0), |all| {
        (all.len() as u64, all.iter().map(|s| s.events).sum::<u64>())
    })
}

// ---------------------------------------------------------------- library

/// Every file under `root`, relative, `/`-separated. Hidden files and
/// our own temporaries are not part of a library.
fn walk(root: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(prefix)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.ends_with(".tmp-sync") {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(root, &rel, out),
            Ok(kind) if kind.is_file() => out.push(rel),
            _ => {}
        }
    }
}

/// The per-file facts, cached by (size, mtime): hashing gigabytes on
/// every sync would make it the slowest part of starting the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Cached {
    size: u64,
    mtime_ms: u64,
    entry: Entry,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The facts a conflict needs, read from the file itself.
fn describe(path: &Path, name: &str, entry: &mut Entry) {
    let facts = || read_json(path);
    if version_of(name).is_some() {
        entry.created_ms = facts().and_then(|v| v.pointer("/provenance/created_ms")?.as_u64());
    } else if name == "song.json" {
        if let Some(doc) = facts() {
            entry.song_id = doc
                .pointer("/identity/song_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            entry.imported_at = doc
                .pointer("/lifecycle/imported_at")
                .and_then(Value::as_u64);
            entry.updated_at = doc.pointer("/lifecycle/updated_at").and_then(Value::as_u64);
        }
    } else if name == POINTER {
        entry.points_to = facts().and_then(|v| v.get("active")?.as_str().map(str::to_owned));
    }
}

/// The library as a manifest (no tombstones yet).
fn scan(data: &Path, write_cache: bool) -> Result<Manifest, String> {
    let root = library_root(data);
    let cache_path = sync_dir(data).join("state").join("hashes.json");
    let cache: BTreeMap<String, Cached> = read_json(&cache_path)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let mut files = Vec::new();
    walk(&root, "", &mut files);
    let mut manifest = Manifest::default();
    let mut fresh = BTreeMap::new();
    for rel in files {
        let path = root.join(&rel);
        let meta = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0));
        let entry = match cache.get(&rel) {
            Some(c) if c.size == meta.len() && c.mtime_ms == mtime_ms => c.entry.clone(),
            _ => {
                let mut entry = Entry {
                    hash: sha256_file(&path)?,
                    size: meta.len(),
                    mtime_ms,
                    created_ms: None,
                    song_id: None,
                    imported_at: None,
                    updated_at: None,
                    points_to: None,
                };
                let name = rel.rsplit('/').next().unwrap_or(&rel);
                describe(&path, name, &mut entry);
                entry
            }
        };
        fresh.insert(
            rel.clone(),
            Cached {
                size: meta.len(),
                mtime_ms,
                entry: entry.clone(),
            },
        );
        manifest.files.insert(rel, entry);
    }
    if write_cache {
        write_json(
            &cache_path,
            &serde_json::to_value(&fresh).unwrap_or_default(),
        )?;
    }
    Ok(manifest)
}

// ---------------------------------------------------------------- snapshot

/// The local state as one document — what a device publishes.
fn local_snapshot(data: &Path, settings: &Path, device: &str) -> Value {
    let settings = read_json(settings).unwrap_or(Value::Null);
    json!({
        "format": FORMAT,
        "device": device,
        "written_ms": now_ms(),
        "players": read_json(&data.join("players.json")).unwrap_or(Value::Null),
        "history": std::fs::read_to_string(data.join("history.jsonl")).unwrap_or_default(),
        "scores": read_json(&data.join("scores.json")).unwrap_or(Value::Null),
        "achievements": read_json(&data.join("achievements.json")).unwrap_or(Value::Null),
        "settings": beatbyte_sync::settings::shared_part(&settings),
        "import_index": read_json(&library_root(data).join("imported-hashes.json"))
            .unwrap_or(Value::Null),
    })
}

// ---------------------------------------------------------------- merge

/// The manifest this device published last — the reference its
/// deletions are measured against.
fn last_manifest(data: &Path) -> Manifest {
    read_json(&sync_dir(data).join("state").join("last-manifest.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// A database file and its write-ahead companions.
fn remove_db(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

/// What merging one other device did.
#[derive(Debug, Default)]
struct Merged {
    notes: Vec<String>,
    fetched: usize,
    moved: usize,
    deleted: usize,
}

fn is_game_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "beatbyte"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Fetch the blobs `hashes` from the hub into `inbox`, each verified.
fn fetch_blobs(hub: &Hub, hashes: &BTreeSet<String>, inbox: &Path) -> Result<(), String> {
    std::fs::create_dir_all(inbox).map_err(|e| format!("{}: {e}", inbox.display()))?;
    let wanted: Vec<&String> = hashes.iter().filter(|h| !inbox.join(h).exists()).collect();
    if !wanted.is_empty() {
        let list = inbox.join(".wanted");
        let body: String = wanted.iter().map(|h| format!("{h}\n")).collect();
        std::fs::write(&list, body).map_err(|e| e.to_string())?;
        let from = format!("{}/", hub.rsync_path("blobs"));
        let files_from = format!("--files-from={}", list.display());
        let partial = "--partial-dir=.rsync-partial";
        rsync(&[&files_from, partial, &from, &inbox.to_string_lossy()])?;
        let _ = std::fs::remove_file(&list);
    }
    for hash in hashes {
        let path = inbox.join(hash);
        let got = sha256_file(&path)?;
        if got != *hash {
            let _ = std::fs::remove_file(&path);
            return Err(format!("blob {hash} arrived as {got}; nothing was changed"));
        }
    }
    Ok(())
}

/// Carry out a library plan on this device.
fn apply_library(
    data: &Path,
    hub: &Hub,
    actions: &[Action],
    merged: &mut Merged,
) -> Result<(), String> {
    let root = library_root(data);
    let inbox = sync_dir(data).join("inbox");
    let hashes: BTreeSet<String> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Fetch { hash, .. } => Some(hash.clone()),
            _ => None,
        })
        .collect();
    fetch_blobs(hub, &hashes, &inbox)?;
    // Moves first: a version renamed out of the way frees its name for
    // the other device's content.
    for action in actions {
        if let Action::Move { from, to } = action {
            let (from, to) = (root.join(from), root.join(to));
            if to.exists() {
                return Err(format!("{} is in the way of a rename", to.display()));
            }
            std::fs::rename(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
            merged.moved += 1;
        }
    }
    for action in actions {
        match action {
            Action::Fetch { path, hash } => {
                let bytes = std::fs::read(inbox.join(hash)).map_err(|e| e.to_string())?;
                write_atomic(&root.join(path), &bytes)?;
                merged.fetched += 1;
            }
            Action::Delete { path } => {
                let file = root.join(path);
                std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                if let Some(dir) = file.parent() {
                    let _ = std::fs::remove_dir(dir); // only if empty
                }
                merged.deleted += 1;
            }
            Action::Point { path, active } => {
                write_atomic(
                    &root.join(path),
                    json!({ "active": active }).to_string().as_bytes(),
                )?;
            }
            Action::Move { .. } => {}
        }
    }
    let _ = std::fs::remove_dir_all(&inbox);
    Ok(())
}

/// Merge one other device's published state into this one.
#[allow(clippy::too_many_lines)] // one pass over the kinds of data, in order
fn merge_device(
    data: &Path,
    settings_path: &Path,
    hub: &Hub,
    remote_dir: &Path,
    dry_run: bool,
) -> Result<Merged, String> {
    let mut merged = Merged::default();
    let snapshot = read_json(&remote_dir.join("snapshot.json"))
        .ok_or_else(|| format!("{}: no snapshot", remote_dir.display()))?;
    if snapshot.get("format").and_then(Value::as_u64) != Some(FORMAT) {
        return Err(format!(
            "{}: snapshot format {:?}, this build knows {FORMAT} — update this device",
            remote_dir.display(),
            snapshot.get("format")
        ));
    }

    // Players first: every other rule needs the remaps.
    let local_roster: Roster = read_json(&data.join("players.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let remote_roster: Roster =
        serde_json::from_value(snapshot["players"].clone()).unwrap_or_default();
    let players = beatbyte_sync::players::merge(&local_roster, &remote_roster);
    merged.notes.extend(players.notes.iter().cloned());

    // The library: its song-id remap feeds the scores and telemetry.
    // With the deletions since the last publish as tombstones — a song
    // deleted here must not come straight back from the other copy.
    let local_manifest = scan(data, !dry_run)?.with_deletions_since(&last_manifest(data), now_ms());
    let remote_manifest: Manifest = read_json(&remote_dir.join("library.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let plan = beatbyte_sync::library::plan(&local_manifest, &remote_manifest);
    merged.notes.extend(plan.notes.iter().cloned());

    let history = beatbyte_sync::history::merge(
        &std::fs::read_to_string(data.join("history.jsonl")).unwrap_or_default(),
        &players.local,
        snapshot["history"].as_str().unwrap_or_default(),
        &players.remote,
    );
    let empty_board = json!({"version": beatbyte_sync::scores::VERSION, "records": []});
    let local_scores = read_json(&data.join("scores.json")).unwrap_or_else(|| empty_board.clone());
    let remote_scores = if snapshot["scores"].is_null() {
        empty_board
    } else {
        snapshot["scores"].clone()
    };
    let scores = beatbyte_sync::scores::merge(&local_scores, &remote_scores, &plan.song_remap)?;
    let achievements = beatbyte_sync::achievements::merge(
        &read_json(&data.join("achievements.json")).unwrap_or(json!({})),
        &players.local,
        &snapshot["achievements"],
        &players.remote,
    );
    let settings = beatbyte_sync::settings::merge(
        &read_json(settings_path).unwrap_or(json!({})),
        &snapshot["settings"],
    );
    let mut index: BTreeSet<String> = BTreeSet::new();
    for list in [
        read_json(&library_root(data).join("imported-hashes.json")),
        Some(snapshot["import_index"].clone()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(items) = list.as_array() {
            index.extend(items.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }

    merged.notes.push(format!(
        "history +{} (enriched {}), scores improved {}, settings taken {:?}, library: {} to fetch",
        history.added,
        history.enriched,
        scores.improved,
        settings.taken,
        plan.actions
            .iter()
            .filter(|a| matches!(a, Action::Fetch { .. }))
            .count()
    ));
    if dry_run {
        return Ok(merged);
    }

    // Files first (the part that can fail on the network), then the
    // documents that describe them.
    apply_library(data, hub, &plan.actions, &mut merged)?;

    // Telemetry: rows, never the file.
    let remote_db = remote_dir.join("telemetry.db");
    if remote_db.exists() {
        let work = sync_dir(data).join("work.db");
        remove_db(&work);
        std::fs::copy(&remote_db, &work).map_err(|e| e.to_string())?;
        // Opening migrates the copy to this build's schema.
        let other = beatbyte_telemetry::Store::open(&work).map_err(|e| e.to_string())?;
        let theirs = beatbyte_telemetry::merge::summaries(&other).map_err(|e| e.to_string())?;
        drop(other);
        let mut store = beatbyte_telemetry::Store::open(&data.join("telemetry.db"))
            .map_err(|e| e.to_string())?;
        let mine = beatbyte_telemetry::merge::summaries(&store).map_err(|e| e.to_string())?;
        let convert = |list: &[beatbyte_telemetry::merge::Summary]| -> Vec<beatbyte_sync::telemetry::Summary> {
            list.iter()
                .map(|s| beatbyte_sync::telemetry::Summary {
                    uid: s.uid.clone(),
                    ended: s.ended,
                    events: s.events,
                })
                .collect()
        };
        let takes: Vec<beatbyte_telemetry::merge::Take> =
            beatbyte_sync::telemetry::plan(&convert(&mine), &convert(&theirs))
                .into_iter()
                .map(|action| match action {
                    beatbyte_sync::telemetry::Action::Insert(uid) => {
                        beatbyte_telemetry::merge::Take {
                            uid,
                            replace: false,
                        }
                    }
                    beatbyte_sync::telemetry::Action::Replace(uid) => {
                        beatbyte_telemetry::merge::Take { uid, replace: true }
                    }
                })
                .collect();
        let remaps = beatbyte_telemetry::merge::Remaps {
            local_players: players.local.clone(),
            remote_players: players.remote.clone(),
            songs: plan.song_remap.clone(),
        };
        let report = beatbyte_telemetry::merge::apply(&mut store, &work, &takes, &remaps)
            .map_err(|e| e.to_string())?;
        drop(store);
        remove_db(&work);
        merged.notes.push(format!(
            "telemetry: {} inserted, {} replaced, {} events",
            report.inserted, report.replaced, report.events
        ));
    }

    write_json(
        &data.join("players.json"),
        &serde_json::to_value(&players.roster).map_err(|e| e.to_string())?,
    )?;
    write_atomic(&data.join("history.jsonl"), history.text.as_bytes())?;
    write_json(&data.join("scores.json"), &scores.board)?;
    write_json(&data.join("achievements.json"), &achievements)?;
    write_json(settings_path, &settings.settings)?;
    if !index.is_empty() {
        write_json(
            &library_root(data).join("imported-hashes.json"),
            &Value::from(index.into_iter().collect::<Vec<_>>()),
        )?;
    }
    Ok(merged)
}

// ---------------------------------------------------------------- the run

struct Lock<'a> {
    hub: &'a Hub,
    held: bool,
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        if self.held {
            let _ = self.hub.shell("rm -rf lock");
        }
    }
}

fn take_lock<'a>(hub: &'a Hub, device: &str) -> Result<Lock<'a>, String> {
    let now = now_ms();
    let script = format!(
        "mkdir -p devices blobs && if mkdir lock 2>/dev/null; then echo {} > lock/owner; echo TAKEN; \
         else cat lock/owner 2>/dev/null; fi",
        quote(&format!("{device} {now}"))
    );
    let answer = hub.shell(&script)?;
    if answer.trim() == "TAKEN" {
        return Ok(Lock { hub, held: true });
    }
    let since = answer
        .split_whitespace()
        .nth(1)
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(0);
    if now.saturating_sub(since) > STALE_LOCK_MS {
        eprintln!("sync: breaking a stale lock ({})", answer.trim());
        hub.shell("rm -rf lock")?;
        return take_lock(hub, device);
    }
    Err(format!(
        "another sync holds the hub ({}); try again in a minute",
        answer.trim()
    ))
}

/// The whole sync; a report on success.
///
/// # Errors
/// On anything that stops it — the game running, the hub unreachable,
/// a blob that does not verify. Every write is atomic, and a sync that
/// failed half-way is completed by the next one.
#[allow(clippy::too_many_lines)] // the steps, in order
pub fn sync(options: &Options) -> Result<String, String> {
    let data = &options.data;
    if options.check_game && is_game_running() {
        return Err("the game is running — close it first (it rewrites its files on exit)".into());
    }
    let mut config = load_config(data, options.dry_run)?;
    let spec = options
        .hub
        .clone()
        .or_else(|| config.hub.clone())
        .ok_or("no hub yet — pass --hub (e.g. raspi5:beatbyte-hub)")?;
    let hub = Hub::parse(&spec);
    let before = counts(data);
    let mut out = vec![
        format!("device {} · hub {}", config.device, hub.spec()),
        format!("before: {before}"),
    ];

    let _lock = if options.dry_run {
        None
    } else {
        Some(take_lock(&hub, &config.device)?)
    };

    // Pull every other device's folder.
    let listing = hub.shell("ls -1 devices 2>/dev/null || true")?;
    let others: Vec<String> = listing
        .lines()
        .map(str::trim)
        .filter(|d| !d.is_empty() && *d != config.device)
        .map(str::to_owned)
        .collect();
    for device in &others {
        let local = sync_dir(data).join("remote").join(device);
        std::fs::create_dir_all(&local).map_err(|e| e.to_string())?;
        let from = format!("{}/", hub.rsync_path(&format!("devices/{device}")));
        rsync(&["--delete", &from, &local.to_string_lossy()])?;
        if !local.join("snapshot.json").exists() {
            out.push(format!("{device}: nothing published yet"));
            continue;
        }
        let merged = merge_device(data, &options.settings, &hub, &local, options.dry_run)?;
        out.push(format!(
            "from {device}: {} fetched, {} moved, {} deleted",
            merged.fetched, merged.moved, merged.deleted
        ));
        out.extend(merged.notes.into_iter().map(|n| format!("  {n}")));
    }
    if others.is_empty() {
        out.push("no other device on the hub yet".to_owned());
    }
    if options.dry_run {
        out.push("dry run: nothing written".to_owned());
        return Ok(out.join("\n"));
    }

    // Publish. Blobs first, the snapshot last.
    let state = sync_dir(data).join("state");
    let manifest = scan(data, true)?.with_deletions_since(&last_manifest(data), now_ms());
    let on_hub: BTreeSet<String> = hub
        .shell("ls -1 blobs")?
        .lines()
        .map(str::to_owned)
        .collect();
    let outbox = sync_dir(data).join("outbox");
    let _ = std::fs::remove_dir_all(&outbox);
    std::fs::create_dir_all(&outbox).map_err(|e| e.to_string())?;
    let mut uploads = 0usize;
    for (rel, entry) in &manifest.files {
        let target = outbox.join(&entry.hash);
        if on_hub.contains(&entry.hash) || target.exists() {
            continue;
        }
        let source = library_root(data).join(rel);
        // A hard link costs nothing on the same volume; a copy is the
        // fallback.
        if std::fs::hard_link(&source, &target).is_err() {
            std::fs::copy(&source, &target).map_err(|e| e.to_string())?;
        }
        uploads += 1;
    }
    if uploads > 0 {
        let to = format!("{}/", hub.rsync_path("blobs"));
        rsync(&[
            "--partial-dir=.rsync-partial",
            &format!("{}/", outbox.display()),
            &to,
        ])?;
    }
    let _ = std::fs::remove_dir_all(&outbox);

    let staging = sync_dir(data).join("publish");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let live_db = data.join("telemetry.db");
    if live_db.exists() {
        beatbyte_telemetry::merge::snapshot(&live_db, &staging.join("telemetry.db"))
            .map_err(|e| e.to_string())?;
    }
    write_json(
        &staging.join("library.json"),
        &serde_json::to_value(&manifest).map_err(|e| e.to_string())?,
    )?;
    let dest = format!("{}/", hub.rsync_path(&format!("devices/{}", config.device)));
    hub.shell(&format!(
        "mkdir -p {}",
        quote(&format!("devices/{}", config.device))
    ))?;
    rsync(&[&format!("{}/", staging.display()), &dest])?;
    write_json(
        &staging.join("snapshot.json"),
        &local_snapshot(data, &options.settings, &config.device),
    )?;
    rsync(&[&staging.join("snapshot.json").to_string_lossy(), &dest])?;
    let _ = std::fs::remove_dir_all(&staging);
    write_json(
        &state.join("last-manifest.json"),
        &serde_json::to_value(&manifest).map_err(|e| e.to_string())?,
    )?;

    if options.models {
        let models = data.join("models");
        std::fs::create_dir_all(&models).map_err(|e| e.to_string())?;
        let remote = format!("{}/", hub.rsync_path("models"));
        hub.shell("mkdir -p models")?;
        rsync(&[
            "--ignore-existing",
            &format!("{}/", models.display()),
            &remote,
        ])?;
        rsync(&[
            "--ignore-existing",
            &remote,
            &format!("{}/", models.display()),
        ])?;
        out.push("models: carried (existing files kept)".to_owned());
    }

    config.hub = Some(spec);
    write_json(
        &sync_dir(data).join("device.json"),
        &serde_json::to_value(&config).map_err(|e| e.to_string())?,
    )?;
    out.push(format!("published: {uploads} new blob(s)"));
    out.push(format!("after:  {}", counts(data)));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests;
