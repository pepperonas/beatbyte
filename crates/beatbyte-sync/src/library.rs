//! The library: song folders, file by file, by content.
//!
//! Each device describes its library as a [`Manifest`]: every file by
//! its path under the library root, its SHA-256, its size and mtime,
//! plus the few facts a conflict needs (a chart's creation time, a
//! document's song id, a pointer's target), and a TOMBSTONE for every
//! file it deleted since the last sync — without tombstones the other
//! device's copy would bring a deleted song straight back.
//!
//! The plan turns two manifests into actions on the local side. It
//! never touches a file; the command line does, atomically, while the
//! game is closed. Both devices compute mirror-image plans from the
//! same pair of manifests and end on the same files — that
//! convergence is the property the tests check, by applying both.
//!
//! The rules:
//! - a file one side has and the other does not is copied — unless the
//!   other side DELETED exactly that content (tombstone, same hash):
//!   then the deletion travels instead;
//! - same path, same content: nothing;
//! - same path, different content:
//!   - a **chart version** (`chart.json`, `chart.vN.json`) keeps BOTH:
//!     the one created first (provenance `created_ms`, then hash) keeps
//!     the name, the other becomes the next free version, its analysis
//!     sidecar (`.context.json`) along with it — a redesign made on each
//!     device is two versions, not one lost;
//!   - the **pointer** (`chart-active.json`) names the version created
//!     last, after the renames above;
//!   - the **document** (`song.json`): two different song ids for one
//!     folder keep the OLDER id (earlier `imported_at`) and report a
//!     song-id remap for everything keyed by the other; the same id in
//!     two versions keeps the one updated last;
//!   - **anything else** (audio, lyrics, derived sidecars) keeps the
//!     newer file (mtime, then hash).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// One file in a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// SHA-256 of the content, lowercase hex.
    pub hash: String,
    /// Bytes.
    pub size: u64,
    /// Modification time, Unix ms.
    pub mtime_ms: u64,
    /// A chart version's provenance `created_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<u64>,
    /// A document's song id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song_id: Option<String>,
    /// A document's `lifecycle.imported_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<u64>,
    /// A document's `lifecycle.updated_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    /// A pointer's target file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points_to: Option<String>,
}

/// A file a device deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    /// The content that was deleted.
    pub hash: String,
    /// When the deletion was noticed, Unix ms.
    pub deleted_ms: u64,
}

/// A device's library, as it describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Path under the library root (`/`-separated) → file.
    pub files: BTreeMap<String, Entry>,
    /// Path → what was deleted there.
    #[serde(default)]
    pub tombstones: BTreeMap<String, Tombstone>,
}

impl Manifest {
    /// Total bytes.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.files.values().map(|e| e.size).sum()
    }

    /// The tombstones for files that were in `previous` and are not
    /// here now, added to the ones already carried. Pure — tested.
    #[must_use]
    pub fn with_deletions_since(mut self, previous: &Manifest, now_ms: u64) -> Manifest {
        for (path, entry) in &previous.files {
            if !self.files.contains_key(path) {
                self.tombstones.entry(path.clone()).or_insert(Tombstone {
                    hash: entry.hash.clone(),
                    deleted_ms: now_ms,
                });
            }
        }
        for (path, stone) in &previous.tombstones {
            self.tombstones
                .entry(path.clone())
                .or_insert_with(|| stone.clone());
        }
        // A tombstone for a file that exists again is stale.
        let files = &self.files;
        self.tombstones.retain(|path, _| !files.contains_key(path));
        self
    }
}

/// One thing to do to the local library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write the blob `hash` at `path`, replacing what is there.
    Fetch {
        /// Where.
        path: String,
        /// Which content.
        hash: String,
    },
    /// Move a local file (a chart version that lost its name).
    Move {
        /// From.
        from: String,
        /// To.
        to: String,
    },
    /// Delete a local file the other device deleted.
    Delete {
        /// Which.
        path: String,
    },
    /// Write a pointer naming `active`.
    Point {
        /// The pointer file.
        path: String,
        /// The version it names.
        active: String,
    },
}

/// The plan for the local side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// In order: moves before fetches into the freed names.
    pub actions: Vec<Action>,
    /// Song id → the id it becomes (a folder with two documents).
    pub song_remap: BTreeMap<String, String>,
    /// The tombstones the local side carries on.
    pub tombstones: BTreeMap<String, Tombstone>,
    /// Lines for the report.
    pub notes: Vec<String>,
}

/// The name of the pointer file.
pub const POINTER: &str = "chart-active.json";

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// A chart version's number: `chart.json` is 1, `chart.vN.json` is N.
#[must_use]
pub fn version_of(name: &str) -> Option<u32> {
    if name == "chart.json" {
        return Some(1);
    }
    name.strip_prefix("chart.v")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

fn version_name(n: u32) -> String {
    if n == 1 {
        "chart.json".to_owned()
    } else {
        format!("chart.v{n}.json")
    }
}

/// The analysis sidecar of a chart file.
fn context_of(chart: &str) -> String {
    let (dir, name) = split(chart);
    let stem = name.strip_suffix(".json").unwrap_or(name);
    format!("{dir}/{stem}.context.json")
}

fn is_context(name: &str) -> bool {
    name.ends_with(".context.json")
}

/// Build the plan for the local side. Pure — tested.
#[must_use]
#[allow(clippy::too_many_lines)] // one rule table, read top to bottom
pub fn plan(local: &Manifest, remote: &Manifest) -> Plan {
    let mut out = Plan::default();
    let mut moves = Vec::new();
    let mut fetches = Vec::new();

    // Folder → version numbers either side uses, so a new name is free
    // on both devices.
    let mut used: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for path in local.files.keys().chain(remote.files.keys()) {
        let (dir, name) = split(path);
        if let Some(n) = version_of(name) {
            used.entry(dir.to_owned()).or_default().insert(n);
        }
    }
    // (side is local?, dir, old name) → new name, for pointers.
    let mut renamed: BTreeMap<(bool, String, String), String> = BTreeMap::new();
    // Context sidecars travel with their chart when it moves.
    let mut handled: BTreeSet<String> = BTreeSet::new();

    let paths: BTreeSet<&String> = local.files.keys().chain(remote.files.keys()).collect();
    for path in &paths {
        let path = path.as_str();
        let (dir, name) = split(path);
        let (mine, theirs) = (local.files.get(path), remote.files.get(path));
        match (mine, theirs) {
            (None, Some(r)) => {
                let deleted_here = local.tombstones.get(path).is_some_and(|t| t.hash == r.hash);
                if !deleted_here && name != POINTER {
                    fetches.push(Action::Fetch {
                        path: path.to_owned(),
                        hash: r.hash.clone(),
                    });
                }
            }
            (Some(l), None) => {
                if remote
                    .tombstones
                    .get(path)
                    .is_some_and(|t| t.hash == l.hash)
                {
                    out.actions.push(Action::Delete {
                        path: path.to_owned(),
                    });
                    out.notes
                        .push(format!("library: {path} deleted on the other device"));
                }
            }
            (Some(l), Some(r)) if l.hash == r.hash => {}
            (Some(l), Some(r)) => {
                if name == POINTER {
                    continue; // resolved below, after the renames
                }
                if let Some(n) = version_of(name) {
                    let order = |e: &Entry| (e.created_ms.unwrap_or(0), e.hash.clone());
                    let local_keeps = order(l) <= order(r);
                    let set = used.entry(dir.to_owned()).or_default();
                    let fresh = set.iter().max().copied().unwrap_or(n) + 1;
                    set.insert(fresh);
                    let new_name = version_name(fresh);
                    let new_path = format!("{dir}/{new_name}");
                    let context = context_of(path);
                    let new_context = context_of(&new_path);
                    if local_keeps {
                        fetches.push(Action::Fetch {
                            path: new_path.clone(),
                            hash: r.hash.clone(),
                        });
                        if let Some(rc) = remote.files.get(&context) {
                            fetches.push(Action::Fetch {
                                path: new_context,
                                hash: rc.hash.clone(),
                            });
                        }
                        renamed.insert((false, dir.to_owned(), name.to_owned()), new_name.clone());
                    } else {
                        moves.push(Action::Move {
                            from: path.to_owned(),
                            to: new_path.clone(),
                        });
                        if local.files.contains_key(&context) {
                            moves.push(Action::Move {
                                from: context.clone(),
                                to: new_context,
                            });
                        }
                        fetches.push(Action::Fetch {
                            path: path.to_owned(),
                            hash: r.hash.clone(),
                        });
                        if let Some(rc) = remote.files.get(&context) {
                            fetches.push(Action::Fetch {
                                path: context.clone(),
                                hash: rc.hash.clone(),
                            });
                        }
                        renamed.insert((true, dir.to_owned(), name.to_owned()), new_name.clone());
                    }
                    handled.insert(context);
                    out.notes.push(format!(
                        "library: {path} differs on the two devices; both kept, the later one as {new_name}"
                    ));
                    continue;
                }
                if is_context(name) && handled.contains(path) {
                    continue;
                }
                if name == "song.json" && l.song_id != r.song_id {
                    let age = |e: &Entry| (e.imported_at.unwrap_or(u64::MAX), e.song_id.clone());
                    let (keep, lose) = if age(l) <= age(r) { (l, r) } else { (r, l) };
                    if let (Some(keep_id), Some(lose_id)) = (&keep.song_id, &lose.song_id) {
                        out.song_remap.insert(lose_id.clone(), keep_id.clone());
                        out.notes.push(format!(
                            "library: {dir} had two song ids; {lose_id} is now {keep_id}"
                        ));
                    }
                    if std::ptr::eq(keep, r) {
                        fetches.push(Action::Fetch {
                            path: path.to_owned(),
                            hash: r.hash.clone(),
                        });
                    }
                    continue;
                }
                let newer = |e: &Entry| {
                    if name == "song.json" {
                        (e.updated_at.unwrap_or(0), e.hash.clone())
                    } else {
                        (e.mtime_ms, e.hash.clone())
                    }
                };
                if newer(r) > newer(l) {
                    fetches.push(Action::Fetch {
                        path: path.to_owned(),
                        hash: r.hash.clone(),
                    });
                }
            }
            (None, None) => {}
        }
    }

    // Pointers: each side's target after the renames; the version
    // created last wins.
    let dirs: BTreeSet<String> = paths
        .iter()
        .filter(|p| split(p).1 == POINTER)
        .map(|p| split(p).0.to_owned())
        .collect();
    for dir in dirs {
        let path = format!("{dir}/{POINTER}");
        let target = |is_local: bool, manifest: &Manifest| -> Option<String> {
            let named = manifest.files.get(&path)?.points_to.clone()?;
            Some(
                renamed
                    .get(&(is_local, dir.clone(), named.clone()))
                    .cloned()
                    .unwrap_or(named),
            )
        };
        let created = |name: &str| -> (u64, String) {
            // Which file ends up under this name: one that moved TO it
            // (from either side), else the remote one if the local one
            // moved AWAY, else whatever stands there.
            let moved_to = |is_local: bool| {
                renamed
                    .iter()
                    .find(|((side, d, _), new)| *side == is_local && *d == dir && *new == name)
                    .map(|((_, _, old), _)| format!("{dir}/{old}"))
            };
            let full = format!("{dir}/{name}");
            let local_moved_away = renamed.contains_key(&(true, dir.clone(), name.to_owned()));
            let entry = moved_to(true)
                .and_then(|p| local.files.get(&p))
                .or_else(|| moved_to(false).and_then(|p| remote.files.get(&p)))
                .or_else(|| {
                    if local_moved_away {
                        remote.files.get(&full)
                    } else {
                        local.files.get(&full).or_else(|| remote.files.get(&full))
                    }
                });
            (
                entry.and_then(|e| e.created_ms).unwrap_or(0),
                name.to_owned(),
            )
        };
        let mine = target(true, local);
        let theirs = target(false, remote);
        let chosen = match (&mine, &theirs) {
            (Some(a), Some(b)) => {
                if created(b) > created(a) {
                    b.clone()
                } else {
                    a.clone()
                }
            }
            (Some(a), None) => a.clone(),
            (None, Some(b)) => b.clone(),
            (None, None) => continue,
        };
        let stale = local
            .files
            .get(&path)
            .and_then(|e| e.points_to.as_ref())
            .is_none_or(|p| *p != chosen);
        if stale {
            fetches.push(Action::Point {
                path,
                active: chosen,
            });
        }
    }

    // Tombstones travel on: the union, the newer per path, and none
    // for a file the merged library still holds.
    let mut stones = local.tombstones.clone();
    for (path, stone) in &remote.tombstones {
        let keep = stones
            .get(path)
            .is_none_or(|have| stone.deleted_ms > have.deleted_ms);
        if keep {
            stones.insert(path.clone(), stone.clone());
        }
    }
    let survives: BTreeSet<String> = fetches
        .iter()
        .filter_map(|a| match a {
            Action::Fetch { path, .. } => Some(path.clone()),
            _ => None,
        })
        .chain(local.files.keys().cloned())
        .collect();
    let deleted: BTreeSet<String> = out
        .actions
        .iter()
        .filter_map(|a| match a {
            Action::Delete { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    stones.retain(|path, _| !survives.contains(path) || deleted.contains(path));
    out.tombstones = stones;

    out.actions.extend(moves);
    out.actions.extend(fetches);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, mtime: u64) -> Entry {
        Entry {
            hash: hash.to_owned(),
            size: 10,
            mtime_ms: mtime,
            created_ms: None,
            song_id: None,
            imported_at: None,
            updated_at: None,
            points_to: None,
        }
    }

    fn chart(hash: &str, created: u64) -> Entry {
        Entry {
            created_ms: Some(created),
            ..entry(hash, 0)
        }
    }

    fn pointer(to: &str) -> Entry {
        Entry {
            points_to: Some(to.to_owned()),
            ..entry(&format!("ptr-{to}"), 0)
        }
    }

    fn manifest(files: &[(&str, Entry)]) -> Manifest {
        Manifest {
            files: files
                .iter()
                .map(|(p, e)| ((*p).to_owned(), e.clone()))
                .collect(),
            tombstones: BTreeMap::new(),
        }
    }

    /// Apply a plan to a side's files (path → hash, pointers as
    /// "->name"), taking fetched content from the other side's
    /// manifest. Only for tests: what the command line does to disk.
    fn apply(side: &Manifest, other: &Manifest, plan: &Plan) -> BTreeMap<String, String> {
        let mut files: BTreeMap<String, String> = side
            .files
            .iter()
            .map(|(p, e)| {
                let content = e
                    .points_to
                    .as_ref()
                    .map_or_else(|| e.hash.clone(), |t| format!("->{t}"));
                (p.clone(), content)
            })
            .collect();
        let _ = other;
        for action in &plan.actions {
            match action {
                Action::Move { from, to } => {
                    if let Some(v) = files.remove(from) {
                        files.insert(to.clone(), v);
                    }
                }
                Action::Fetch { path, hash } => {
                    files.insert(path.clone(), hash.clone());
                }
                Action::Delete { path } => {
                    files.remove(path);
                }
                Action::Point { path, active } => {
                    files.insert(path.clone(), format!("->{active}"));
                }
            }
        }
        files
    }

    /// Both devices apply their plans; they must end identical.
    fn converge(a: &Manifest, b: &Manifest) -> BTreeMap<String, String> {
        let on_a = apply(a, b, &plan(a, b));
        let on_b = apply(b, a, &plan(b, a));
        assert_eq!(on_a, on_b, "the two devices diverged");
        on_a
    }

    #[test]
    fn a_song_only_one_device_has_is_copied() {
        let a = manifest(&[("imported/x/a.m4a", entry("h1", 1))]);
        let b = Manifest::default();
        let files = converge(&a, &b);
        assert_eq!(
            files.get("imported/x/a.m4a").map(String::as_str),
            Some("h1")
        );
    }

    /// ⚠️ A deletion travels; without the tombstone the other device's
    /// copy would bring the song straight back.
    #[test]
    fn a_deleted_song_stays_deleted_on_both() {
        let previous = manifest(&[("imported/x/a.m4a", entry("h1", 1))]);
        let a = Manifest::default().with_deletions_since(&previous, 99);
        assert_eq!(a.tombstones["imported/x/a.m4a"].hash, "h1");
        let b = manifest(&[("imported/x/a.m4a", entry("h1", 1))]);
        let files = converge(&a, &b);
        assert!(files.is_empty(), "the deleted song came back: {files:?}");
    }

    /// A file changed on the other device AFTER the deletion here is
    /// new content, and it is kept.
    #[test]
    fn a_tombstone_only_deletes_the_content_it_names() {
        let mut a = Manifest::default();
        a.tombstones.insert(
            "imported/x/a.lrc".to_owned(),
            Tombstone {
                hash: "old".to_owned(),
                deleted_ms: 5,
            },
        );
        let b = manifest(&[("imported/x/a.lrc", entry("new", 9))]);
        let files = converge(&a, &b);
        assert_eq!(
            files.get("imported/x/a.lrc").map(String::as_str),
            Some("new")
        );
    }

    /// ⚠️ The same version regenerated on both devices: both kept, the
    /// one created first under the name, the other as the next free
    /// version with its sidecar — identically on both devices.
    #[test]
    fn one_version_generated_twice_becomes_two_versions() {
        let a = manifest(&[
            ("imported/x/chart.json", chart("base", 1)),
            ("imported/x/chart.v2.json", chart("mine", 200)),
            ("imported/x/chart.v2.context.json", entry("ctx-mine", 0)),
        ]);
        let b = manifest(&[
            ("imported/x/chart.json", chart("base", 1)),
            ("imported/x/chart.v2.json", chart("theirs", 100)),
            ("imported/x/chart.v2.context.json", entry("ctx-theirs", 0)),
        ]);
        let files = converge(&a, &b);
        assert_eq!(
            files["imported/x/chart.v2.json"], "theirs",
            "the earlier keeps the name"
        );
        assert_eq!(files["imported/x/chart.v3.json"], "mine");
        assert_eq!(files["imported/x/chart.v3.context.json"], "ctx-mine");
        assert_eq!(files["imported/x/chart.v2.context.json"], "ctx-theirs");
    }

    /// The pointer follows the renames and names the version created
    /// last.
    #[test]
    fn the_pointer_names_the_newest_version_after_renames() {
        let a = manifest(&[
            ("imported/x/chart.v2.json", chart("mine", 200)),
            ("imported/x/chart-active.json", pointer("chart.v2.json")),
        ]);
        let b = manifest(&[
            ("imported/x/chart.v2.json", chart("theirs", 100)),
            ("imported/x/chart-active.json", pointer("chart.v2.json")),
        ]);
        let files = converge(&a, &b);
        // Device A's v2 (created later) became v3, and it is the newest.
        assert_eq!(files["imported/x/chart-active.json"], "->chart.v3.json");
    }

    /// Two documents for one folder: the older song id wins, and the
    /// other is reported for everything keyed by it.
    #[test]
    fn two_song_ids_keep_the_older_and_remap_the_other() {
        let doc = |id: &str, imported: u64| Entry {
            song_id: Some(id.to_owned()),
            imported_at: Some(imported),
            ..entry(&format!("doc-{id}"), 0)
        };
        let a = manifest(&[("imported/x/song.json", doc("bb_new", 900))]);
        let b = manifest(&[("imported/x/song.json", doc("bb_old", 100))]);
        let here = plan(&a, &b);
        assert_eq!(
            here.song_remap.get("bb_new").map(String::as_str),
            Some("bb_old")
        );
        assert_eq!(
            plan(&b, &a).song_remap.get("bb_new").map(String::as_str),
            Some("bb_old")
        );
        let files = converge(&a, &b);
        assert_eq!(files["imported/x/song.json"], "doc-bb_old");
    }

    /// Anything else: the newer file.
    #[test]
    fn a_differing_file_keeps_the_newer() {
        let a = manifest(&[("imported/x/a.lrc", entry("old", 1))]);
        let b = manifest(&[("imported/x/a.lrc", entry("new", 2))]);
        let files = converge(&a, &b);
        assert_eq!(files["imported/x/a.lrc"], "new");
    }

    /// Once converged, a second sync has nothing to do.
    #[test]
    fn a_second_sync_does_nothing() {
        let a = manifest(&[
            ("imported/x/chart.json", chart("base", 1)),
            ("imported/x/song.m4a", entry("audio", 1)),
        ]);
        assert!(plan(&a, &a).actions.is_empty());
    }

    #[test]
    fn version_names_round_trip() {
        assert_eq!(version_of("chart.json"), Some(1));
        assert_eq!(version_of("chart.v7.json"), Some(7));
        assert_eq!(version_of("chart.v7.context.json"), None);
        assert_eq!(version_of("chart-active.json"), None);
        assert_eq!(version_name(1), "chart.json");
        assert_eq!(version_name(7), "chart.v7.json");
    }
}
