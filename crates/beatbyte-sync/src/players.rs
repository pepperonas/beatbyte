//! Players: one roster from two, and the id remaps every other rule
//! applies before it merges.
//!
//! A player's identity is its id, but two ways exist for two devices
//! to disagree about what an id means, and each needs a remap rather
//! than a merge:
//!
//! 1. **An id collision.** The per-device counter this replaced gave
//!    two players made offline on two machines the same id. Same id,
//!    different `created_ms` → two people; the one created LATER (ties:
//!    by name) gets a fresh id from
//!    [`beatbyte_core::player::device_independent_id`].
//! 2. **One person, two ids.** The same name made on both machines
//!    (the user's rule, ADR-0021: same name = one person, as the
//!    roster already allows one name per person). The player created
//!    FIRST keeps its id; the other's id maps onto it.
//!
//! Both are decided from the two rosters alone, so both devices
//! compute the same remaps and converge. The result carries a remap
//! for EACH side, because rule 2 can rename a local id too.
//!
//! The same player on both sides (same id, same creation) keeps the
//! version changed last ([`Player::changed_ms`]); a preference is the
//! one place where "the newer choice" is the honest rule, and it is
//! per player, not per file.

use std::collections::BTreeMap;

use beatbyte_core::player::{Player, PlayerId, Roster, device_independent_id, same_name};

/// Old id → new id, for one side.
pub type Remap = BTreeMap<PlayerId, PlayerId>;

/// What merging two rosters produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    /// The one roster both devices will hold.
    pub roster: Roster,
    /// How the LOCAL side's ids change. Apply it to everything local
    /// that names a player before merging.
    pub local: Remap,
    /// How the REMOTE side's ids change.
    pub remote: Remap,
    /// Human-readable lines for the sync report.
    pub notes: Vec<String>,
}

/// Apply a remap to one id.
#[must_use]
pub fn map(remap: &Remap, id: PlayerId) -> PlayerId {
    remap.get(&id).copied().unwrap_or(id)
}

/// Which of two versions of one player to keep: the later change;
/// on an exact tie, the one whose serialization sorts first, so both
/// devices pick the same.
fn newer(a: &Player, b: &Player) -> Player {
    match a.changed_ms().cmp(&b.changed_ms()) {
        std::cmp::Ordering::Greater => a.clone(),
        std::cmp::Ordering::Less => b.clone(),
        std::cmp::Ordering::Equal => {
            let key = |p: &Player| serde_json::to_string(p).unwrap_or_default();
            if key(a) <= key(b) {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}

/// Merge two rosters. Pure — tested.
///
/// `selected` stays the LOCAL device's choice (remapped): who is
/// sitting at this machine is not something another machine knows.
#[must_use]
pub fn merge(local: &Roster, remote: &Roster) -> Merged {
    let mut notes = Vec::new();
    let mut remote_map = Remap::new();
    let mut local_map = Remap::new();

    // 1. Id collisions: same id, different creation → two people.
    //    The later one moves, on whichever side it is.
    let mut remote_players: Vec<Player> = remote.players.clone();
    let mut local_players: Vec<Player> = local.players.clone();
    let taken = |players: &[Player], other: &[Player], id: PlayerId| {
        players.iter().chain(other).any(|p| p.id == id)
    };
    for r in &mut remote_players {
        let Some(l) = local_players.iter_mut().find(|l| l.id == r.id) else {
            continue;
        };
        if l.created_ms == r.created_ms {
            continue;
        }
        let remote_moves = (r.created_ms, &r.name) > (l.created_ms, &l.name);
        let mover: &mut Player = if remote_moves { r } else { l };
        let old = mover.id;
        let mut fresh = device_independent_id(mover.created_ms, &mover.name);
        while fresh == old || taken(&local.players, &remote.players, fresh) {
            fresh += 1;
        }
        mover.id = fresh;
        let side = if remote_moves {
            &mut remote_map
        } else {
            &mut local_map
        };
        side.insert(old, fresh);
        notes.push(format!(
            "players: id {old} named two people; \"{}\" is now {fresh}",
            mover.name
        ));
    }

    // 2. One person, two ids (same name): the first created keeps
    //    its id, the other maps onto it.
    let mut all: Vec<(bool, Player)> = local_players
        .iter()
        .map(|p| (true, p.clone()))
        .chain(remote_players.iter().map(|p| (false, p.clone())))
        .collect();
    all.sort_by_key(|a| (a.1.created_ms, a.1.id));
    let mut kept: Vec<Player> = Vec::new();
    for (is_local, player) in all {
        let same_person = kept
            .iter()
            .position(|k| k.id == player.id || same_name(&k.name, &player.name));
        match same_person {
            Some(index) => {
                let keeper_id = kept[index].id;
                if keeper_id != player.id {
                    let side = if is_local {
                        &mut local_map
                    } else {
                        &mut remote_map
                    };
                    // Compose with a step-1 remap of the same side.
                    let original = side
                        .iter()
                        .find(|(_, v)| **v == player.id)
                        .map_or(player.id, |(k, _)| *k);
                    side.insert(original, keeper_id);
                    notes.push(format!(
                        "players: \"{}\" exists on both devices; {} merged into {keeper_id}",
                        player.name, player.id
                    ));
                    // The kept record is the first one created; its
                    // name and preferences follow the newer change.
                    let mut merged = newer(&kept[index], &player);
                    merged.id = keeper_id;
                    merged.created_ms = kept[index].created_ms;
                    kept[index] = merged;
                } else {
                    let merged = newer(&kept[index], &player);
                    kept[index] = merged;
                }
            }
            None => kept.push(player),
        }
    }
    kept.sort_by_key(|p| (p.created_ms, p.id));

    let mut roster = local.clone();
    roster.players = kept;
    roster.selected = local.selected.map(|id| map(&local_map, id));
    // A device that had nobody of its own takes the first player it
    // receives — the game's own rule for the first player to exist
    // (nobody builds a roster to then pick from it). Without it the
    // first run on a freshly synced device belongs to no one.
    if roster.selected.is_none() && local.players.is_empty() {
        roster.selected = roster.players.first().map(|p| p.id);
    }
    roster.absorb_counters(remote);
    Merged {
        roster,
        local: local_map,
        remote: remote_map,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: PlayerId, name: &str, created_ms: u64) -> Player {
        Player {
            id,
            name: name.to_owned(),
            created_ms,
            colour: 0,
            preferred_difficulty: None,
            updated_ms: None,
        }
    }

    fn roster(players: Vec<Player>) -> Roster {
        let mut r = Roster::default();
        r.selected = players.first().map(|p| p.id);
        r.players = players;
        r
    }

    fn ids(m: &Merged) -> Vec<(PlayerId, String)> {
        m.roster
            .players
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }

    /// The same player on both devices is one player, and nothing moves.
    #[test]
    fn the_same_player_on_both_sides_is_one() {
        let a = roster(vec![player(1, "Martin", 100)]);
        let m = merge(&a, &a);
        assert_eq!(ids(&m), vec![(1, "Martin".to_owned())]);
        assert!(m.local.is_empty() && m.remote.is_empty());
    }

    /// ⚠️ The case the counter caused: both devices made "their" id 2
    /// for two different people offline. Both survive; the later one
    /// moves, and its side's remap says where.
    #[test]
    fn one_id_for_two_people_moves_the_later_one() {
        let local = roster(vec![player(1, "Martin", 100), player(2, "Kim", 200)]);
        let remote = roster(vec![player(1, "Martin", 100), player(2, "Lea", 300)]);
        let m = merge(&local, &remote);
        assert_eq!(m.roster.players.len(), 3);
        assert!(m.local.is_empty(), "the earlier one moved: {:?}", m.local);
        let lea = m
            .roster
            .players
            .iter()
            .find(|p| p.name == "Lea")
            .expect("Lea");
        assert_ne!(lea.id, 2);
        assert_eq!(m.remote.get(&2), Some(&lea.id));
        // Symmetric: merged the other way round, the same person moves.
        let back = merge(&remote, &local);
        assert_eq!(back.local.get(&2), Some(&lea.id));
        assert!(back.remote.is_empty());
    }

    /// The user's rule: the same name on two devices is one person, and
    /// the first created keeps the id — from either side.
    #[test]
    fn one_name_on_two_devices_is_one_person_the_first_created() {
        let local = roster(vec![player(500, "Anna", 5_000)]);
        let remote = roster(vec![player(300, "anna", 3_000)]);
        let m = merge(&local, &remote);
        assert_eq!(m.roster.players.len(), 1, "{:?}", ids(&m));
        assert_eq!(m.roster.players[0].id, 300);
        assert_eq!(m.local.get(&500), Some(&300), "the local id must follow");
        assert!(m.remote.is_empty());
        assert_eq!(m.roster.selected, Some(300), "the selection follows too");
    }

    /// A rename on one device beats the older version on the other.
    #[test]
    fn the_newer_change_of_one_player_wins() {
        let mut renamed = player(1, "Martin P", 100);
        renamed.updated_ms = Some(9_000);
        let local = roster(vec![player(1, "Martin", 100)]);
        let remote = roster(vec![renamed]);
        assert_eq!(merge(&local, &remote).roster.players[0].name, "Martin P");
        assert_eq!(merge(&remote, &local).roster.players[0].name, "Martin P");
    }

    /// Order-independence and idempotence: the two devices end on the
    /// same roster, and merging it again changes nothing.
    #[test]
    fn both_devices_converge_and_a_second_merge_changes_nothing() {
        let local = roster(vec![player(1, "Martin", 100), player(2, "Kim", 200)]);
        let remote = roster(vec![
            player(1, "Martin", 100),
            player(2, "Lea", 300),
            player(9, "KIM", 150),
        ]);
        let here = merge(&local, &remote);
        let there = merge(&remote, &local);
        assert_eq!(ids(&here), ids(&there));
        let again = merge(&here.roster, &there.roster);
        assert_eq!(ids(&again), ids(&here));
        assert!(again.local.is_empty() && again.remote.is_empty());
    }

    /// The WHOLE file converges, not only the ids: the counters too.
    /// Only `selected` is a device's own (who sits at this machine).
    /// Comparing ids alone let two devices keep different counters —
    /// the end-to-end sync test found it.
    #[test]
    fn both_devices_end_with_the_same_roster_file() {
        let mut a = Roster::default();
        a.add("Martin", 1_000).expect("adds");
        let mut b = Roster::default();
        b.add("martin", 2_000).expect("adds");
        b.add("Kim", 3_000).expect("adds");
        let file = |r: &Roster| {
            let mut v = serde_json::to_value(r).expect("roster");
            v["selected"] = serde_json::Value::Null;
            v
        };
        let here = merge(&a, &b).roster;
        let there = merge(&b, &a).roster;
        assert_eq!(file(&here), file(&there));
    }

    /// A fresh device selects the first player it receives; a device
    /// with players of its own keeps its choice, even if that is none.
    #[test]
    fn a_fresh_device_selects_the_player_it_receives() {
        let remote = roster(vec![player(5, "Martin", 100)]);
        assert_eq!(merge(&Roster::default(), &remote).roster.selected, Some(5));
        let mut mine = roster(vec![player(9, "Kim", 50)]);
        mine.selected = None;
        assert_eq!(merge(&mine, &remote).roster.selected, None);
    }

    #[test]
    fn a_map_leaves_unknown_ids_alone() {
        let mut remap = Remap::new();
        remap.insert(2, 7);
        assert_eq!(map(&remap, 2), 7);
        assert_eq!(map(&remap, 3), 3);
    }
}
