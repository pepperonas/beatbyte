//! The play log (`history.jsonl`): the union of both logs, each run
//! once.
//!
//! **No new id field.** A run is already identified by what it is:
//! the millisecond it started, the song (title and artist) and the
//! difficulty. Two devices would have to start the same song on the
//! same difficulty in the same millisecond to collide. A line a
//! device has and the other does not is added; a line both have is
//! kept once — the RICHER copy, because the roster's one-time adoption
//! rewrote old lines on one device only (it added `player`), and that
//! attribution must not be lost to the other device's bare copy.
//!
//! Lines that do not parse are carried through byte for byte (the
//! game's own rule for its log), each distinct one once. Output is
//! ordered by start time; the game sorts runs itself, and a fixed
//! order makes both devices' files identical.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::players::{Remap, map};

/// What identifies a run.
type Key = (u64, String, String, String);

fn key(line: &Value) -> Option<Key> {
    Some((
        line.get("started_ms")?.as_u64()?,
        line.get("title")?.as_str()?.to_owned(),
        line.get("artist")?.as_str()?.to_owned(),
        line.get("difficulty")?.as_str()?.to_owned(),
    ))
}

/// How much a line says: the number of fields that are set, counting
/// the ones inside nested objects, so an attributed line outweighs an
/// unattributed copy of the same run.
fn richness(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Object(map) => map.values().map(|v| 1 + richness(v)).sum(),
        Value::Array(items) => items.iter().map(richness).sum(),
        _ => 1,
    }
}

/// Rewrite the player ids a line names.
fn remap_line(line: &mut Value, remap: &Remap) -> bool {
    let mut changed = false;
    let mut fix = |slot: &mut Value| {
        if let Some(id) = slot.as_u64() {
            let to = map(remap, id);
            if to != id {
                *slot = Value::from(to);
                changed = true;
            }
        }
    };
    if let Some(player) = line.get_mut("player") {
        fix(player);
    }
    if let Some(Value::Array(parts)) = line.get_mut("co_players") {
        for part in parts {
            if let Some(player) = part.get_mut("player") {
                fix(player);
            }
        }
    }
    changed
}

/// A line as read: its text, and what it parses to.
struct Line {
    text: String,
    value: Option<Value>,
}

fn read(text: &str, remap: &Remap) -> Vec<Line> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut value: Option<Value> = serde_json::from_str(l).ok();
            let mut text = l.to_owned();
            if let Some(v) = value.as_mut()
                && remap_line(v, remap)
            {
                text = serde_json::to_string(v).unwrap_or(text);
            }
            Line { text, value }
        })
        .collect()
}

/// What merging two logs did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// The merged log, one run per line, ending in a newline.
    pub text: String,
    /// Runs the local log did not have.
    pub added: usize,
    /// Runs both had, where the remote copy said more and was kept.
    pub enriched: usize,
}

/// Merge two logs, each with its side's player remap applied first.
/// Pure — tested.
#[must_use]
pub fn merge(local: &str, local_remap: &Remap, remote: &str, remote_remap: &Remap) -> Merged {
    let mut runs: BTreeMap<Key, (usize, String, bool)> = BTreeMap::new();
    let mut unparsed: Vec<String> = Vec::new();
    let (mut added, mut enriched) = (0usize, 0usize);
    for (is_local, lines) in [
        (true, read(local, local_remap)),
        (false, read(remote, remote_remap)),
    ] {
        for line in lines {
            let Some(value) = line.value.as_ref() else {
                if !unparsed.contains(&line.text) {
                    unparsed.push(line.text);
                }
                continue;
            };
            let Some(k) = key(value) else {
                if !unparsed.contains(&line.text) {
                    unparsed.push(line.text);
                }
                continue;
            };
            let rich = richness(value);
            match runs.get(&k) {
                None => {
                    if !is_local {
                        added += 1;
                    }
                    runs.insert(k, (rich, line.text, is_local));
                }
                // The richer copy wins; on a tie the text that sorts
                // first, so both devices keep the same bytes.
                Some((have, text, _)) => {
                    let better = rich > *have || (rich == *have && line.text < *text);
                    if better {
                        if !is_local {
                            enriched += 1;
                        }
                        runs.insert(k, (rich, line.text, is_local));
                    }
                }
            }
        }
    }
    let mut text = String::new();
    for (_, (_, line, _)) in runs {
        text.push_str(&line);
        text.push('\n');
    }
    for line in unparsed {
        text.push_str(&line);
        text.push('\n');
    }
    Merged {
        text,
        added,
        enriched,
    }
}

/// How many runs a log holds (lines that parse to a run).
#[must_use]
pub fn count(text: &str) -> usize {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| key(v).is_some())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ms: u64, title: &str, player: Option<u64>) -> String {
        let mut v = serde_json::json!({
            "title": title, "artist": "A", "difficulty": "hard",
            "started_ms": ms, "score": 100
        });
        if let Some(p) = player {
            v["player"] = Value::from(p);
        }
        serde_json::to_string(&v).expect("json")
    }

    fn log(lines: &[String]) -> String {
        lines.iter().map(|l| format!("{l}\n")).collect()
    }

    fn none() -> Remap {
        Remap::new()
    }

    /// Both devices played offline: every run once, none twice.
    #[test]
    fn two_offline_logs_become_their_union() {
        let shared = run(1, "Shared", Some(1));
        let local = log(&[shared.clone(), run(2, "Here", Some(1))]);
        let remote = log(&[shared, run(3, "There", Some(1))]);
        let m = merge(&local, &none(), &remote, &none());
        assert_eq!(count(&m.text), 3);
        assert_eq!(m.added, 1);
        assert!(m.text.contains("Here") && m.text.contains("There"));
    }

    /// ⚠️ The adoption added `player` to old lines on ONE device. The
    /// same run arriving bare from the other device must not win the
    /// attribution back.
    #[test]
    fn the_attributed_copy_of_a_run_beats_a_bare_one() {
        let bare = log(&[run(1, "Old", None)]);
        let attributed = log(&[run(1, "Old", Some(1))]);
        for (local, remote) in [(&bare, &attributed), (&attributed, &bare)] {
            let m = merge(local, &none(), remote, &none());
            assert_eq!(count(&m.text), 1);
            assert!(m.text.contains("\"player\":1"), "{}", m.text);
        }
    }

    /// A player remap rewrites the lines of its side, co-players too.
    #[test]
    fn a_remap_follows_the_player_into_every_line() {
        let mut line: Value = serde_json::from_str(&run(5, "Duo", Some(2))).expect("json");
        line["co_players"] = serde_json::json!([{"player": 2, "score": 1, "accuracy": 1.0}]);
        let remote = format!("{}\n", serde_json::to_string(&line).expect("json"));
        let mut remap = Remap::new();
        remap.insert(2, 77);
        let m = merge("", &none(), &remote, &remap);
        assert!(m.text.contains("\"player\":77"), "{}", m.text);
        assert!(!m.text.contains("\"player\":2"), "{}", m.text);
    }

    /// Lines that do not parse survive, each once.
    #[test]
    fn a_line_that_does_not_parse_is_carried_once() {
        let local = format!("{}\nnot json\n", run(1, "A", None));
        let remote = "not json\n".to_owned();
        let m = merge(&local, &none(), &remote, &none());
        assert_eq!(m.text.matches("not json").count(), 1);
    }

    /// Both devices end on identical bytes, and a second merge changes
    /// nothing.
    #[test]
    fn both_orders_give_the_same_file_and_merging_again_is_a_no_op() {
        let a = log(&[run(1, "X", Some(1)), run(4, "Y", None)]);
        let b = log(&[run(4, "Y", Some(1)), run(2, "Z", Some(1))]);
        let ab = merge(&a, &none(), &b, &none());
        let ba = merge(&b, &none(), &a, &none());
        assert_eq!(ab.text, ba.text);
        let again = merge(&ab.text, &none(), &ba.text, &none());
        assert_eq!(again.text, ab.text);
        assert_eq!(again.added, 0);
    }
}
