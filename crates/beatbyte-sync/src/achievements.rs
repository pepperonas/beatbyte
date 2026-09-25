//! Achievements (`achievements.json`): the union, and for each one
//! the EARLIEST date it was earned.
//!
//! The file maps a player id to `{ "at": { achievement: ms } }`. The
//! game re-derives what is earned from the whole play log on every
//! pass (ADR-0017), so after the logs merge the set follows by itself;
//! the one stored fact is WHEN, and an achievement earned first on the
//! other device was earned then. Player ids follow their side's remap,
//! and two ids mapped onto one player merge their achievements the
//! same way.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::players::{Remap, map};

/// Merge two files, each with its side's remap applied. Pure — tested.
#[must_use]
pub fn merge(local: &Value, local_remap: &Remap, remote: &Value, remote_remap: &Remap) -> Value {
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut extra: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for (file, remap) in [(local, local_remap), (remote, remote_remap)] {
        let Some(players) = file.as_object() else {
            continue;
        };
        for (player, entry) in players {
            let key = player
                .parse::<u64>()
                .map_or_else(|_| player.clone(), |id| map(remap, id).to_string());
            let earned = out.entry(key.clone()).or_default();
            if let Some(at) = entry.get("at").and_then(Value::as_object) {
                for (achievement, when) in at {
                    let Some(when) = when.as_u64() else {
                        continue;
                    };
                    earned
                        .entry(achievement.clone())
                        .and_modify(|have| *have = (*have).min(when))
                        .or_insert(when);
                }
            }
            // Anything else the entry carries rides along, the local
            // copy first.
            if let Some(object) = entry.as_object() {
                let slot = extra.entry(key).or_default();
                for (field, value) in object {
                    if field != "at" {
                        slot.entry(field.clone()).or_insert_with(|| value.clone());
                    }
                }
            }
        }
    }
    let mut result = Map::new();
    for (player, earned) in out {
        let mut entry = extra.remove(&player).unwrap_or_default();
        let at: Map<String, Value> = earned
            .into_iter()
            .map(|(k, v)| (k, Value::from(v)))
            .collect();
        entry.insert("at".to_owned(), Value::Object(at));
        result.insert(player, Value::Object(entry));
    }
    Value::Object(result)
}

/// How many (player, achievement) pairs a file holds.
#[must_use]
pub fn count(file: &Value) -> usize {
    file.as_object().map_or(0, |players| {
        players
            .values()
            .filter_map(|e| e.get("at").and_then(Value::as_object))
            .map(Map::len)
            .sum()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn none() -> Remap {
        Remap::new()
    }

    #[test]
    fn the_union_keeps_the_earliest_date() {
        let local = json!({"1": {"at": {"combo_50": 500, "diff_hard": 100}}});
        let remote = json!({"1": {"at": {"combo_50": 300, "cal_monday": 900}}});
        let m = merge(&local, &none(), &remote, &none());
        assert_eq!(m["1"]["at"]["combo_50"], 300, "not the earliest");
        assert_eq!(m["1"]["at"]["diff_hard"], 100);
        assert_eq!(m["1"]["at"]["cal_monday"], 900);
        assert_eq!(count(&m), 3);
    }

    #[test]
    fn a_remapped_player_brings_their_achievements_along() {
        let local = json!({"1": {"at": {"a": 50}}});
        let remote = json!({"2": {"at": {"a": 10, "b": 20}}});
        let mut remap = Remap::new();
        remap.insert(2, 1);
        let m = merge(&local, &none(), &remote, &remap);
        assert_eq!(m, json!({"1": {"at": {"a": 10, "b": 20}}}));
    }

    #[test]
    fn both_orders_agree_and_merging_again_changes_nothing() {
        let a = json!({"1": {"at": {"x": 5}}, "9": {"at": {"y": 1}}});
        let b = json!({"1": {"at": {"x": 3, "z": 7}}});
        let ab = merge(&a, &none(), &b, &none());
        assert_eq!(ab, merge(&b, &none(), &a, &none()));
        assert_eq!(merge(&ab, &none(), &ab, &none()), ab);
    }
}
