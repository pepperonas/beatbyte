//! Who is playing: the local roster.
//!
//! Lives here rather than in the game crate for the reason the play
//! history and the telemetry schema do — the game WRITES the roster
//! and the statistics READ it, so the model has exactly one
//! definition and neither side can drift from the other.
//!
//! No accounts and no network, like the scoreboard beside it: a
//! roster is a handful of names on one machine. What it adds over
//! "player one" is an **identity that outlives a run**, so a year of
//! play can be attributed, compared and drawn.

use serde::{Deserialize, Serialize};

use crate::difficulty::Difficulty;

/// A player's stable handle.
///
/// Ids are what the history stores, never names: renaming yourself
/// must not orphan the runs you already played. Assigned from a
/// counter that only ever climbs, so a deleted player's id is never
/// handed to someone else and old log lines cannot be re-attributed
/// by accident.
pub type PlayerId = u64;

/// A player id that is unique across DEVICES, not just on one
/// (ADR-0021): the creation millisecond shifted up ten bits, with ten
/// bits of the case-folded name below it.
///
/// ⚠️ It replaced a per-device counter, which gave two players made
/// offline on two machines the same id — and every run, record and
/// achievement of one would have been credited to the other when the
/// machines met. From a timestamp and a name, two devices only agree
/// on an id for the same name in the same millisecond, which is the
/// same person (the roster allows one name per person). No randomness:
/// the core is deterministic, and a hash of the name is enough.
///
/// Small ids from the old counter (`1`, `2`, …) can never come out of
/// this, so they stay what they are. The result fits an SQLite
/// `INTEGER` with room to spare (under 2^62).
#[must_use]
pub fn device_independent_id(created_ms: u64, name: &str) -> PlayerId {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ((created_ms & ((1 << 51) - 1)) << 10) | (hash & 0x3ff)
}

/// The longest name the roster accepts.
///
/// A name is drawn in one row of a list and printed in headers; past
/// this it stops being a name and starts being a layout problem.
pub const NAME_MAX: usize = 24;

/// How many accent colours the roster cycles through.
///
/// Matches the game's player palette (P1–P4). A fifth player wraps
/// to the first colour rather than being refused — the colour is a
/// garnish, the id is the identity.
pub const COLOURS: usize = 4;

/// One player on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    /// Stable id; never reused.
    pub id: PlayerId,
    /// Display name, already normalized by [`normalize_name`].
    pub name: String,
    /// Unix milliseconds when the player was created.
    pub created_ms: u64,
    /// Index into the game's player palette, `0..COLOURS`.
    pub colour: usize,
    /// Last difficulty this player chose in song select.
    ///
    /// Song-agnostic: the next song list opens on this step when the
    /// chart offers it. `None` means "never chosen" — the browser
    /// keeps its own default (Medium). A chart that lacks the step
    /// falls back for the session without clearing this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_difficulty: Option<Difficulty>,
    /// Unix milliseconds of the last change to this player (a rename,
    /// a new preferred difficulty). `None`: unchanged since creation.
    /// When two devices hold different versions of one player, the
    /// newer change wins (ADR-0021).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_ms: Option<u64>,
}

impl Player {
    /// When this player last changed: the stamp, or its creation.
    #[must_use]
    pub fn changed_ms(&self) -> u64 {
        self.updated_ms.unwrap_or(self.created_ms)
    }
}

/// Why a name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but whitespace (or control characters).
    Empty,
    /// Another player already answers to this name.
    Taken,
}

impl NameError {
    /// A line to show the player. Pure.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            NameError::Empty => "A NAME CANNOT BE BLANK",
            NameError::Taken => "THAT NAME IS TAKEN",
        }
    }
}

/// A typed name, cleaned up: control characters dropped, whitespace
/// squeezed and trimmed, capped at [`NAME_MAX`] characters.
///
/// Characters, not bytes — a cap that splits a multi-byte character
/// would panic, which is the trap the telemetry's comment cap
/// documents. Pure — tested.
#[must_use]
pub fn normalize_name(typed: &str) -> String {
    let cleaned: String = typed
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    squeezed.chars().take(NAME_MAX).collect()
}

/// Whether two names are the same person as far as the roster is
/// concerned. Case-insensitive: "Martin" and "martin" in one list
/// would be a trap, not a feature. Pure — tested.
#[must_use]
pub fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Everyone who plays on this machine, and who is playing now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    /// The players, in creation order.
    pub players: Vec<Player>,
    /// Who the next run is attributed to, if anyone.
    #[serde(default)]
    pub selected: Option<PlayerId>,
    /// The next id to hand out. Climbs only.
    #[serde(default)]
    next_id: PlayerId,
    /// Whether the one-time adoption of unattributed runs has
    /// happened (see [`Roster::adopts_orphans`]).
    #[serde(default)]
    adopted: bool,
}

impl Roster {
    /// Add a player, returning their new id.
    ///
    /// # Errors
    /// [`NameError::Empty`] for a blank name, [`NameError::Taken`]
    /// when someone already answers to it.
    pub fn add(&mut self, typed: &str, now_ms: u64) -> Result<PlayerId, NameError> {
        let name = normalize_name(typed);
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if self.players.iter().any(|p| same_name(&p.name, &name)) {
            return Err(NameError::Taken);
        }
        // Unique across devices (ADR-0021); an id already in the file
        // — a hand edit, or two names that hash alike in the same
        // millisecond — moves up rather than colliding.
        let mut id = device_independent_id(now_ms, &name);
        while self.players.iter().any(|p| p.id == id) {
            id += 1;
        }
        self.next_id = self.next_id.max(id + 1);
        self.players.push(Player {
            id,
            name,
            created_ms: now_ms,
            colour: self.players.len() % COLOURS,
            preferred_difficulty: None,
            updated_ms: None,
        });
        // The first player to exist is the one playing: nobody
        // creates a roster in order to then pick from it.
        if self.selected.is_none() {
            self.selected = Some(id);
        }
        Ok(id)
    }

    /// Take over another copy's counters, so two devices that merged
    /// their rosters end with the same file (ADR-0021): the id counter
    /// climbs to the larger of the two (and past every id present),
    /// and the one-time adoption counts as done if it happened on
    /// either — it only ever fires when the FIRST player is added, and
    /// a merged roster already has one.
    pub fn absorb_counters(&mut self, other: &Roster) {
        let past_all = self.players.iter().map(|p| p.id + 1).max().unwrap_or(0);
        self.next_id = self.next_id.max(other.next_id).max(past_all);
        self.adopted = self.adopted || other.adopted;
    }

    /// Rename a player in place.
    ///
    /// # Errors
    /// As [`Roster::add`]. Renaming someone to the name they already
    /// have (in any casing) is allowed — it is how you fix casing.
    pub fn rename(&mut self, id: PlayerId, typed: &str) -> Result<(), NameError> {
        let name = normalize_name(typed);
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if self
            .players
            .iter()
            .any(|p| p.id != id && same_name(&p.name, &name))
        {
            return Err(NameError::Taken);
        }
        if let Some(player) = self.players.iter_mut().find(|p| p.id == id) {
            player.name = name;
        }
        Ok(())
    }

    /// Record that a player changed at `now_ms` — call it after a
    /// rename or a new preferred difficulty, so a device that holds an
    /// older version of the player knows this one is newer.
    pub fn stamp(&mut self, id: PlayerId, now_ms: u64) {
        if let Some(player) = self.players.iter_mut().find(|p| p.id == id) {
            player.updated_ms = Some(now_ms);
        }
    }

    /// Make this player the one runs are attributed to. Unknown ids
    /// are ignored rather than clearing the selection.
    pub fn select(&mut self, id: PlayerId) {
        if self.players.iter().any(|p| p.id == id) {
            self.selected = Some(id);
        }
    }

    /// The player a run should be attributed to, if the roster knows
    /// one. `None` means "nobody has been chosen" — a run played
    /// then is logged unattributed rather than guessed at.
    #[must_use]
    pub fn current(&self) -> Option<&Player> {
        self.selected.and_then(|id| self.get(id))
    }

    /// Look a player up.
    #[must_use]
    pub fn get(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// The name behind an id, for a label.
    #[must_use]
    pub fn name_of(&self, id: PlayerId) -> Option<&str> {
        self.get(id).map(|p| p.name.as_str())
    }

    /// How many players are on this machine.
    #[must_use]
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// Whether nobody has been created yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// Whether the next player created should adopt the runs that
    /// were played before the roster existed.
    ///
    /// The play history predates players by many months, and those
    /// runs belong to somebody: without this the first player's
    /// statistics would open empty next to a log full of their own
    /// play. It fires **once**, for the first player only, and the
    /// flag is stored so a later roster edit cannot re-trigger it.
    #[must_use]
    pub const fn adopts_orphans(&self) -> bool {
        !self.adopted && self.players.is_empty()
    }

    /// Record that the adoption has run, whatever it found.
    pub const fn mark_adopted(&mut self) {
        self.adopted = true;
    }

    /// Remember the difficulty this player last chose.
    ///
    /// Unknown ids are ignored. Does not touch other players — each
    /// profile keeps its own preference.
    pub fn set_preferred_difficulty(&mut self, id: PlayerId, difficulty: Difficulty) {
        if let Some(player) = self.players.iter_mut().find(|p| p.id == id) {
            player.preferred_difficulty = Some(difficulty);
        }
    }

    /// The difficulty the current player last chose, if any.
    #[must_use]
    pub fn current_preferred_difficulty(&self) -> Option<Difficulty> {
        self.current().and_then(|p| p.preferred_difficulty)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_cleaned_but_not_mangled() {
        assert_eq!(normalize_name("  Martin  "), "Martin");
        assert_eq!(normalize_name("Martin\tP\nfeffer"), "Martin P feffer");
        assert_eq!(normalize_name("   "), "");
        // The cap counts CHARACTERS: a byte cap would split the "ä"
        // and panic, which is the trap the comment cap documents.
        let long = "ä".repeat(NAME_MAX + 10);
        assert_eq!(normalize_name(&long).chars().count(), NAME_MAX);
    }

    #[test]
    fn the_first_player_is_the_one_playing() {
        let mut roster = Roster::default();
        let id = roster.add("Martin", 1_000).unwrap();
        assert_eq!(roster.selected, Some(id));
        assert_eq!(roster.current().map(|p| p.name.as_str()), Some("Martin"));
        // A second player does NOT steal the selection: adding a
        // friend to the list is not sitting down at the guitar.
        let friend = roster.add("Kim", 2_000).unwrap();
        assert_eq!(roster.selected, Some(id));
        roster.select(friend);
        assert_eq!(roster.selected, Some(friend));
    }

    #[test]
    fn names_are_unique_whatever_the_casing() {
        let mut roster = Roster::default();
        roster.add("Martin", 1).unwrap();
        assert_eq!(roster.add("martin", 2), Err(NameError::Taken));
        assert_eq!(roster.add("  MARTIN ", 3), Err(NameError::Taken));
        assert_eq!(roster.add("   ", 4), Err(NameError::Empty));
        // Fixing your own casing is a rename to "the same" name, and
        // must not be refused as taken.
        let id = roster.players[0].id;
        assert!(roster.rename(id, "MARTIN").is_ok());
        assert_eq!(roster.name_of(id), Some("MARTIN"));
    }

    #[test]
    fn ids_climb_and_are_never_reused() {
        let mut roster = Roster::default();
        let first = roster.add("A", 1).unwrap();
        let second = roster.add("B", 2).unwrap();
        assert!(second > first);
        // A player removed by a hand edit must not give their id to
        // the next arrival — the history still points at it.
        roster.players.retain(|p| p.id != second);
        let third = roster.add("C", 3).unwrap();
        assert!(third > second, "id {third} reused after {second}");
    }

    #[test]
    fn a_hand_edited_file_cannot_make_two_players_share_an_id() {
        // next_id says 1, but a player already holds 7: the counter
        // must jump past them rather than collide.
        let mut roster = Roster {
            players: vec![Player {
                id: 7,
                name: "Old".to_owned(),
                created_ms: 0,
                colour: 0,
                preferred_difficulty: None,
                updated_ms: None,
            }],
            selected: None,
            next_id: 1,
            adopted: false,
        };
        let id = roster.add("New", 1).unwrap();
        assert_ne!(id, 7);
        // And an id the file already holds is stepped past, never
        // shared.
        let taken = device_independent_id(5, "Twin");
        roster.players[0].id = taken;
        let next = roster.add("Twin", 5).unwrap();
        assert_ne!(next, taken, "two players share an id");
    }

    /// ⚠️ ADR-0021: an id from the creation time and the name, never a
    /// per-device counter — two devices adding their first player each
    /// used to both hand out id 1.
    #[test]
    fn a_new_id_is_the_same_on_every_device_and_never_a_counter() {
        let mut here = Roster::default();
        let mut there = Roster::default();
        let a = here.add("Anna", 1_790_000_000_000).unwrap();
        let b = there.add("Ben", 1_790_000_000_001).unwrap();
        assert_ne!(a, b, "two devices' first players share an id");
        assert!(a > 1_000_000, "a counter-sized id: {a}");
        // The same person made at the same moment is the same id.
        assert_eq!(
            device_independent_id(1_790_000_000_000, "anna"),
            device_independent_id(1_790_000_000_000, "ANNA")
        );
        // Ordered by creation, and small legacy ids cannot come out.
        assert!(device_independent_id(2, "x") > device_independent_id(1, "x"));
        assert!(device_independent_id(0, "x") < 1024);
        assert!(device_independent_id(u64::MAX, "x") < (1 << 62));
    }

    #[test]
    fn a_stamp_marks_a_player_as_changed() {
        let mut roster = Roster::default();
        let id = roster.add("Martin", 1_000).unwrap();
        assert_eq!(roster.get(id).unwrap().changed_ms(), 1_000);
        roster.stamp(id, 5_000);
        assert_eq!(roster.get(id).unwrap().changed_ms(), 5_000);
        roster.stamp(id + 99, 9_000);
        assert_eq!(roster.get(id).unwrap().changed_ms(), 5_000);
    }

    #[test]
    fn the_adoption_fires_once_and_only_for_the_first_player() {
        let mut roster = Roster::default();
        assert!(roster.adopts_orphans(), "an empty roster adopts");
        roster.mark_adopted();
        assert!(!roster.adopts_orphans(), "and never a second time");

        let mut fresh = Roster::default();
        fresh.add("Martin", 1).unwrap();
        assert!(
            !fresh.adopts_orphans(),
            "a roster with players does not adopt: those runs are not \
             automatically the next player's"
        );
    }

    #[test]
    fn colours_cycle_rather_than_running_out() {
        let mut roster = Roster::default();
        for n in 0..COLOURS + 2 {
            roster.add(&format!("P{n}"), n as u64).unwrap();
        }
        assert_eq!(roster.players[0].colour, 0);
        assert_eq!(roster.players[COLOURS].colour, 0, "the fifth wraps");
        assert!(roster.players.iter().all(|p| p.colour < COLOURS));
    }

    #[test]
    fn a_roster_survives_a_round_trip_and_an_older_file() {
        let mut roster = Roster::default();
        roster.add("Martin", 1_700_000_000_000).unwrap();
        let json = serde_json::to_string(&roster).unwrap();
        assert_eq!(serde_json::from_str::<Roster>(&json).unwrap(), roster);
        // A file written before `selected`/`adopted` existed still
        // reads: every added field carries a default.
        let older: Roster =
            serde_json::from_str(r#"{"players":[{"id":3,"name":"A","created_ms":0,"colour":0}]}"#)
                .unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(older.selected, None);
        assert_eq!(older.players[0].preferred_difficulty, None);
    }

    #[test]
    fn each_player_keeps_their_own_difficulty_preference() {
        let mut roster = Roster::default();
        let martin = roster.add("Martin", 1).unwrap();
        let kim = roster.add("Kim", 2).unwrap();
        assert_eq!(roster.current_preferred_difficulty(), None);

        roster.set_preferred_difficulty(martin, Difficulty::Hard);
        roster.set_preferred_difficulty(kim, Difficulty::Medium);
        assert_eq!(
            roster.get(martin).unwrap().preferred_difficulty,
            Some(Difficulty::Hard)
        );
        assert_eq!(
            roster.get(kim).unwrap().preferred_difficulty,
            Some(Difficulty::Medium)
        );

        // Selection decides whose preference is "current".
        assert_eq!(roster.selected, Some(martin));
        assert_eq!(
            roster.current_preferred_difficulty(),
            Some(Difficulty::Hard)
        );
        roster.select(kim);
        assert_eq!(
            roster.current_preferred_difficulty(),
            Some(Difficulty::Medium)
        );

        // A fallback for a chart does not clear the stored preference.
        let offered = [Difficulty::Easy, Difficulty::Medium];
        let session = Difficulty::among(Difficulty::Hard, &offered).unwrap();
        assert_eq!(session, Difficulty::Medium);
        assert_eq!(
            roster.get(martin).unwrap().preferred_difficulty,
            Some(Difficulty::Hard),
            "session fallback must not rewrite the profile"
        );

        // Round-trip keeps both preferences (a restart).
        let json = serde_json::to_string(&roster).unwrap();
        let back: Roster = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.get(martin).unwrap().preferred_difficulty,
            Some(Difficulty::Hard)
        );
        assert_eq!(
            back.get(kim).unwrap().preferred_difficulty,
            Some(Difficulty::Medium)
        );
    }
}
