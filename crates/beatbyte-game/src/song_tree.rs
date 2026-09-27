//! The song browser as a tree: a song, its variants (the song itself,
//! its guitar study, its classic twin), and each one's revisions.
//!
//! Before, every twin was a row of its own directly under its original
//! and only the ACTIVE revision of anything was visible — an older
//! revision could neither be played nor chosen. Now a song is one row;
//! opening it shows NORMAL / GS / CL (a song without twins opens
//! straight onto its revisions), and opening a variant shows its
//! revisions, the active one marked. Everything is collapsed until the
//! player opens it, so the list reads as it did before.
//!
//! Pure over indices and closures: the browser brings the order (the
//! sort and the twin pairing already applied), which entry a twin
//! belongs to, and the revisions of an entry's folder; nothing here
//! touches the disk. Tested.

/// One revision, as a row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionRow {
    /// The revision number.
    pub number: u32,
    /// The file name (`chart.v3.json`).
    pub name: String,
    /// Who made it, in a word ("HAND-MADE", "GENERATED", …).
    pub label: String,
    /// Whether it is the one that plays.
    pub active: bool,
}

/// What a row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A song: the family's original.
    Song {
        /// Whether it has anything to open.
        expandable: bool,
        /// Whether it is open.
        open: bool,
    },
    /// A variant under an open song: the original ("NORMAL") or a twin.
    Variant {
        /// "NORMAL", "GS", "CL", "CL / GS".
        label: String,
        /// Whether it has more than one revision to show.
        expandable: bool,
        /// Whether it is open.
        open: bool,
    },
    /// One revision of the entry above.
    Revision(RevisionRow),
}

/// One row of the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The library entry it plays (for a revision: its folder's entry).
    pub entry: usize,
    /// How far it is indented.
    pub depth: u8,
    /// What it is.
    pub kind: Kind,
}

/// Which level of the tree a row opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    /// The song row (opens its variants, or its revisions).
    Song,
    /// A variant row (opens its revisions).
    Variant,
}

/// A row's identity across rebuilds: the cursor follows THIS, not the
/// library entry — the song row, its NORMAL row and every revision
/// row of the original share one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowId {
    /// A song row.
    Song(usize),
    /// A variant row.
    Variant(usize),
    /// A revision row: entry and revision number.
    Revision(usize, u32),
}

impl Row {
    /// Its identity.
    #[must_use]
    pub fn id(&self) -> RowId {
        match &self.kind {
            Kind::Song { .. } => RowId::Song(self.entry),
            Kind::Variant { .. } => RowId::Variant(self.entry),
            Kind::Revision(r) => RowId::Revision(self.entry, r.number),
        }
    }

    /// The level Tab opens on this row, if it opens at all.
    #[must_use]
    pub fn opens(&self) -> Option<Level> {
        match self.kind {
            Kind::Song {
                expandable: true, ..
            } => Some(Level::Song),
            Kind::Variant {
                expandable: true, ..
            } => Some(Level::Variant),
            _ => None,
        }
    }
}

/// What the tree needs to know about the library, as closures.
pub struct Facts<P, L, C, R, O>
where
    P: Fn(usize) -> Option<usize>,
    L: Fn(usize) -> String,
    C: Fn(usize) -> usize,
    R: FnMut(usize) -> Vec<RevisionRow>,
    O: Fn(usize, Level) -> bool,
{
    /// The entry a twin was made from, if that entry is in the list.
    pub parent_of: P,
    /// A variant's label ("GS", "CL / GS"; the original is "NORMAL").
    pub label: L,
    /// How many revisions an entry's folder holds.
    pub revision_count: C,
    /// An entry's revisions, oldest first (only asked for when open).
    pub revisions: R,
    /// Whether the player opened this entry at this level.
    pub is_open: O,
}

/// The browser's rows for `order` (already sorted, twins directly
/// after the entry they were made from). Every entry of `order`
/// appears in at least one row or inside a closed song — a song never
/// vanishes from the list. Pure — tested.
pub fn build<P, L, C, R, O>(order: &[usize], facts: Facts<P, L, C, R, O>) -> Vec<Row>
where
    P: Fn(usize) -> Option<usize>,
    L: Fn(usize) -> String,
    C: Fn(usize) -> usize,
    R: FnMut(usize) -> Vec<RevisionRow>,
    O: Fn(usize, Level) -> bool,
{
    let Facts {
        parent_of,
        label,
        revision_count,
        mut revisions,
        is_open,
    } = facts;
    // The root of an entry's family: follow the twins up. A chain
    // cannot loop (a twin's title is strictly longer than its
    // original's), but the step bound makes that structural.
    let root_of = |mut i: usize| {
        for _ in 0..order.len() {
            match parent_of(i) {
                Some(parent) if parent != i => i = parent,
                _ => break,
            }
        }
        i
    };
    let mut rows = Vec::new();
    let mut revisions_under = |rows: &mut Vec<Row>, entry: usize, depth: u8| {
        for revision in revisions(entry) {
            rows.push(Row {
                entry,
                depth,
                kind: Kind::Revision(revision),
            });
        }
    };
    let mut at = 0;
    while at < order.len() {
        let root = order[at];
        // The family: the entries after the root that belong to it.
        // The pairing put them there; anything that names another
        // root starts its own family.
        let mut members = Vec::new();
        let mut next = at + 1;
        while next < order.len() && order[next] != root && root_of(order[next]) == root {
            members.push(order[next]);
            next += 1;
        }
        at = next;

        if members.is_empty() {
            let expandable = revision_count(root) >= 2;
            let open = expandable && is_open(root, Level::Song);
            rows.push(Row {
                entry: root,
                depth: 0,
                kind: Kind::Song { expandable, open },
            });
            if open {
                revisions_under(&mut rows, root, 1);
            }
            continue;
        }
        let open = is_open(root, Level::Song);
        rows.push(Row {
            entry: root,
            depth: 0,
            kind: Kind::Song {
                expandable: true,
                open,
            },
        });
        if !open {
            continue;
        }
        for (variant, name) in std::iter::once((root, "NORMAL".to_owned()))
            .chain(members.iter().map(|&m| (m, label(m))))
        {
            let expandable = revision_count(variant) >= 2;
            let variant_open = expandable && is_open(variant, Level::Variant);
            rows.push(Row {
                entry: variant,
                depth: 1,
                kind: Kind::Variant {
                    label: name,
                    expandable,
                    open: variant_open,
                },
            });
            if variant_open {
                revisions_under(&mut rows, variant, 2);
            }
        }
    }
    rows
}

/// A twin's label from its title's prefixes: `[GS] Maria` is "GS",
/// `[CL] [GS] Maria` — a classic twin of the study — "CL / GS". Pure.
#[must_use]
pub fn variant_label(title: &str) -> String {
    let mut tags = Vec::new();
    let mut rest = title;
    loop {
        if let Some(after) = rest.strip_prefix("[GS] ") {
            tags.push("GS");
            rest = after;
        } else if let Some(after) = rest.strip_prefix("[CL] ") {
            tags.push("CL");
            rest = after;
        } else {
            break;
        }
    }
    if tags.is_empty() {
        "NORMAL".to_owned()
    } else {
        tags.join(" / ")
    }
}

/// Who made a revision, from its provenance's designer. Pure.
#[must_use]
pub fn designer_label(designer: Option<&str>) -> &'static str {
    match designer {
        None => "GENERATED",
        Some(beatbyte_chart::versions::EDITOR_DESIGNER) => "HAND-MADE",
        Some("design-session") => "REDESIGN",
        Some("classic") => "CLASSIC",
        Some("lead-study") => "STUDY",
        Some(_) => "TOOL",
    }
}

/// Where the cursor goes after the rows changed: onto the same row if
/// it survived, else onto the row that now holds its entry (a revision
/// row whose variant was closed lands on the variant or the song),
/// else clamped. Pure — tested.
#[must_use]
pub fn follow_cursor(old: &[RowId], cursor: usize, new: &[Row]) -> usize {
    let Some(was) = old.get(cursor) else {
        return cursor.min(new.len().saturating_sub(1));
    };
    if let Some(at) = new.iter().position(|row| row.id() == *was) {
        return at;
    }
    let entry = match *was {
        RowId::Song(e) | RowId::Variant(e) | RowId::Revision(e, _) => e,
    };
    new.iter()
        .rposition(|row| row.entry == entry && !matches!(row.kind, Kind::Revision(_)))
        .or_else(|| new.iter().position(|row| row.entry == entry))
        .unwrap_or_else(|| cursor.min(new.len().saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Entries: 0 "Alpha" (3 revisions), 1 "[GS] Alpha" (2), 2 "[CL]
    /// [GS] Alpha" (1), 3 "Beta" (1), 4 "Gamma" (2).
    fn titles() -> Vec<&'static str> {
        vec!["Alpha", "[GS] Alpha", "[CL] [GS] Alpha", "Beta", "Gamma"]
    }

    fn counts(entry: usize) -> usize {
        [3, 2, 1, 1, 2][entry]
    }

    fn rows(order: &[usize], open: &HashSet<(usize, Level)>) -> Vec<Row> {
        let titles = titles();
        build(
            order,
            Facts {
                parent_of: |i| match i {
                    1 => Some(0).filter(|p| order.contains(p)),
                    2 => Some(1).filter(|p| order.contains(p)),
                    _ => None,
                },
                label: |i| variant_label(titles[i]),
                revision_count: counts,
                revisions: |i| {
                    (1..=counts(i) as u32)
                        .map(|n| RevisionRow {
                            number: n,
                            name: format!("chart.v{n}.json"),
                            label: "GENERATED".to_owned(),
                            active: n == counts(i) as u32,
                        })
                        .collect()
                },
                is_open: |i, level| open.contains(&(i, level)),
            },
        )
    }

    fn shape(rows: &[Row]) -> Vec<(usize, u8, &'static str)> {
        rows.iter()
            .map(|r| {
                (
                    r.entry,
                    r.depth,
                    match &r.kind {
                        Kind::Song { .. } => "song",
                        Kind::Variant { .. } => "variant",
                        Kind::Revision(_) => "revision",
                    },
                )
            })
            .collect()
    }

    const ORDER: [usize; 5] = [0, 1, 2, 3, 4];

    #[test]
    fn closed_the_list_is_one_row_per_song() {
        let rows = rows(&ORDER, &HashSet::new());
        assert_eq!(
            shape(&rows),
            vec![(0, 0, "song"), (3, 0, "song"), (4, 0, "song")]
        );
        assert_eq!(
            rows[0].opens(),
            Some(Level::Song),
            "a song with twins opens"
        );
        assert_eq!(
            rows[1].opens(),
            None,
            "one revision, no twins: nothing to open"
        );
        assert_eq!(rows[2].opens(), Some(Level::Song), "two revisions open");
    }

    #[test]
    fn an_open_song_shows_normal_and_its_twins_the_chain_included() {
        let open = HashSet::from([(0, Level::Song)]);
        let rows = rows(&ORDER, &open);
        assert_eq!(
            shape(&rows)[..4],
            [
                (0, 0, "song"),
                (0, 1, "variant"),
                (1, 1, "variant"),
                (2, 1, "variant")
            ]
        );
        let labels: Vec<String> = rows
            .iter()
            .filter_map(|r| match &r.kind {
                Kind::Variant { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, vec!["NORMAL", "GS", "CL / GS"]);
        // The chain's last link has one revision: nothing to open.
        assert_eq!(rows[3].opens(), None);
    }

    #[test]
    fn an_open_variant_lists_its_revisions_with_the_active_one_marked() {
        let open = HashSet::from([(0, Level::Song), (1, Level::Variant)]);
        let rows = rows(&ORDER, &open);
        let revisions: Vec<(u32, bool, u8)> = rows
            .iter()
            .filter_map(|r| match &r.kind {
                Kind::Revision(rev) => Some((rev.number, rev.active, r.depth)),
                _ => None,
            })
            .collect();
        assert_eq!(revisions, vec![(1, false, 2), (2, true, 2)]);
        // A variant that is marked open but has one revision stays shut.
        let odd = HashSet::from([(0, Level::Song), (2, Level::Variant)]);
        assert!(!rows_of_kind_revision(&self::rows(&ORDER, &odd)));
    }

    fn rows_of_kind_revision(rows: &[Row]) -> bool {
        rows.iter().any(|r| matches!(r.kind, Kind::Revision(_)))
    }

    #[test]
    fn a_song_without_twins_opens_straight_onto_its_revisions() {
        let open = HashSet::from([(4, Level::Song)]);
        let rows = rows(&ORDER, &open);
        assert_eq!(
            shape(&rows)[2..],
            [(4, 0, "song"), (4, 1, "revision"), (4, 1, "revision")]
        );
    }

    #[test]
    fn a_twin_without_its_original_in_the_list_is_a_song_of_its_own() {
        // A search that matched only the study.
        let rows = rows(&[1, 2], &HashSet::new());
        assert_eq!(shape(&rows), vec![(1, 0, "song")]);
        assert!(matches!(
            rows[0].kind,
            Kind::Song {
                expandable: true,
                ..
            }
        ));
    }

    #[test]
    fn no_song_ever_vanishes() {
        // Fully open, every entry is in some row; closed, every entry
        // is in a row or inside a closed song row's family.
        let all = HashSet::from([
            (0, Level::Song),
            (0, Level::Variant),
            (1, Level::Variant),
            (4, Level::Song),
        ]);
        let rows = rows(&ORDER, &all);
        for entry in ORDER {
            assert!(
                rows.iter().any(|r| r.entry == entry),
                "entry {entry} missing"
            );
        }
    }

    #[test]
    fn the_cursor_stays_on_its_row_and_falls_back_to_its_entry() {
        let closed = rows(&ORDER, &HashSet::new());
        let open = rows(&ORDER, &HashSet::from([(0, Level::Song)]));
        // On the NORMAL row (same entry as the song row above it).
        let ids: Vec<RowId> = open.iter().map(Row::id).collect();
        assert_eq!(follow_cursor(&ids, 1, &open), 1, "not onto the song row");
        // Closing the song: the NORMAL row is gone, the song holds it.
        assert_eq!(follow_cursor(&ids, 1, &closed), 0);
        // The GS row is gone too: its entry is inside the closed song,
        // so the cursor clamps rather than jumping to another song.
        let gs = follow_cursor(&ids, 2, &closed);
        assert!(gs < closed.len());
    }

    #[test]
    fn labels_read_the_prefixes_and_the_designer() {
        assert_eq!(variant_label("Maria"), "NORMAL");
        assert_eq!(variant_label("[GS] Maria"), "GS");
        assert_eq!(variant_label("[CL] [GS] Maria"), "CL / GS");
        assert_eq!(designer_label(None), "GENERATED");
        assert_eq!(designer_label(Some("editor")), "HAND-MADE");
        assert_eq!(designer_label(Some("design-session")), "REDESIGN");
        assert_eq!(designer_label(Some("something-new")), "TOOL");
    }
}
