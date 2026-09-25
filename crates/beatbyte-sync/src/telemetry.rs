//! Telemetry (`telemetry.db`): rows, never files — decided here, done
//! by `beatbyte-telemetry`.
//!
//! A session is its `uid` (start millisecond plus a scramble; ADR-0018),
//! unique across devices in practice. A session only the remote store
//! has is inserted, with its events and notes; a session both stores
//! have is kept once — the MORE COMPLETE copy: an ended session over an
//! open one, then more events over fewer (a store copied while the run
//! was still recording holds a prefix of it). A full tie keeps the
//! local copy. This module only decides; the SQL lives with the store,
//! which alone may write it.

use std::collections::BTreeMap;

/// What the plan needs to know about one stored session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The session's uid.
    pub uid: String,
    /// Whether it was finished (`ended_ms` set).
    pub ended: bool,
    /// How many events it holds.
    pub events: u64,
}

/// One thing to do to the local store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Copy this remote session, events and notes in.
    Insert(String),
    /// Replace the local copy of this session with the remote one.
    Replace(String),
}

/// The plan. Pure — tested.
#[must_use]
pub fn plan(local: &[Summary], remote: &[Summary]) -> Vec<Action> {
    let here: BTreeMap<&str, &Summary> = local.iter().map(|s| (s.uid.as_str(), s)).collect();
    let mut actions = Vec::new();
    for theirs in remote {
        match here.get(theirs.uid.as_str()) {
            None => actions.push(Action::Insert(theirs.uid.clone())),
            Some(mine) if (theirs.ended, theirs.events) > (mine.ended, mine.events) => {
                actions.push(Action::Replace(theirs.uid.clone()));
            }
            Some(_) => {}
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(uid: &str, ended: bool, events: u64) -> Summary {
        Summary {
            uid: uid.to_owned(),
            ended,
            events,
        }
    }

    #[test]
    fn a_session_only_the_other_device_has_is_inserted() {
        let plan = plan(&[s("a", true, 10)], &[s("a", true, 10), s("b", true, 3)]);
        assert_eq!(plan, vec![Action::Insert("b".to_owned())]);
    }

    /// ⚠️ A store copied mid-run holds an open prefix of a session.
    /// The finished copy wins, and a longer one over a shorter one.
    #[test]
    fn the_more_complete_copy_of_a_session_wins() {
        assert_eq!(
            plan(&[s("a", false, 500)], &[s("a", true, 400)]),
            vec![Action::Replace("a".to_owned())]
        );
        assert_eq!(
            plan(&[s("a", true, 400)], &[s("a", true, 450)]),
            vec![Action::Replace("a".to_owned())]
        );
        assert!(plan(&[s("a", true, 450)], &[s("a", false, 900)]).is_empty());
        assert!(plan(&[s("a", true, 450)], &[s("a", true, 450)]).is_empty());
    }

    #[test]
    fn a_second_merge_has_nothing_to_do() {
        let merged = [s("a", true, 1), s("b", true, 2)];
        assert!(plan(&merged, &merged).is_empty());
    }
}
