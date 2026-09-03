//! Fuzzy song search: how a typed query scores against a title, an
//! artist and a genre.
//!
//! Pure and deterministic — same query, same library, same ranking.
//! The rules, in the order they were needed:
//!
//! - **Words, not phrases.** The query and every field are split into
//!   words; each query word must find SOMETHING (AND), and the entry's
//!   score is the sum of each word's best hit. "queen rhapsody" names
//!   an artist and a title, and a phrase test found nothing.
//! - **Folded on both sides**: case, diacritics ("Sacre" finds
//!   "Sacré"), and apostrophes and punctuation are not letters —
//!   "dont" finds "Don't Stop Believin'", "metallica" finds
//!   "Metallica- Nothing Else Matters".
//! - **Typos are tolerated in proportion to the word.** A query word
//!   of four letters or more may be one edit away from a field word
//!   (a missed, extra, wrong or swapped letter: "smels" → "smells",
//!   "armi" → "army", "luftbalons" → "luftballons"); eight or more,
//!   two edits, and the first letter must agree. Three letters or
//!   fewer must match exactly, or "the" is half the library. (Four
//!   is a trade: "life" also finds "like" — ranked BELOW the exact
//!   hit — but "armi" finding nothing was the reported experience,
//!   and recall wins over a tidy list.)
//! - **Exact beats prefix beats substring beats typo**, so the list
//!   ranks the song the player meant above the songs that merely
//!   resemble the query — and a filter therefore ORDERS the list by
//!   score, with the chosen sort breaking ties.
//! - **Title and artist count double, genre single.** Genre is a
//!   tie-breaker of a column, not what a search is for.

/// One query word's best score against one field word.
pub const EXACT: u32 = 120;
/// The field word begins with the query word.
pub const PREFIX: u32 = 100;
/// The query word occurs inside the field word.
pub const INSIDE: u32 = 80;
/// One edit away (four letters or more).
pub const ONE_EDIT: u32 = 60;
/// Two edits away (eight letters or more).
pub const TWO_EDITS: u32 = 40;

/// Field weights: title and artist are what a search is for.
const TITLE_WEIGHT: u32 = 2;
const ARTIST_WEIGHT: u32 = 2;
const GENRE_WEIGHT: u32 = 1;

/// The searchable words of a text: folded (case, diacritics), with
/// apostrophes dropped and everything else non-alphanumeric treated
/// as a separator. "Don't Stop Believin'" → `dont stop believin`.
#[must_use]
pub fn words(text: &str) -> Vec<String> {
    let folded: String = text
        .chars()
        .flat_map(|c| {
            crate::ui::fold_latin(c).map_or_else(
                || c.to_lowercase().collect::<Vec<_>>(),
                |s| s.chars().collect(),
            )
        })
        .filter(|c| !matches!(c, '\'' | '’' | '‘'))
        .collect();
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Optimal-string-alignment distance: insertions, deletions,
/// substitutions and adjacent transpositions, each costing one.
#[must_use]
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return n.max(m);
    }
    // Three rows suffice for OSA: the transposition looks two back.
    let mut prev2 = vec![0usize; m + 1];
    let mut prev = (0..=m).collect::<Vec<usize>>();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(prev2[j - 2] + 1);
            }
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// How many edits a query word of this length may be off by.
#[must_use]
pub fn allowed_edits(query_len: usize) -> usize {
    match query_len {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// One query word against one field word: the best rule that
/// applies, or 0 for no match.
#[must_use]
pub fn word_score(query: &str, word: &str) -> u32 {
    if query.is_empty() {
        return 0;
    }
    if word == query {
        return EXACT;
    }
    if word.starts_with(query) {
        return PREFIX;
    }
    if word.contains(query) {
        return INSIDE;
    }
    let allowed = allowed_edits(query.chars().count());
    if allowed == 0 {
        return 0;
    }
    // A typo almost never lands on the first letter, and without
    // this rule "gall" is one edit from "all", "call" and "sally":
    // the exact hit still ranked first, but four strangers followed
    // it (seen on the real library).
    if query.chars().next() != word.chars().next() {
        return 0;
    }
    // Against the whole word, and against its prefix of the query's
    // length: a word still being typed with a typo in it ("luftbla")
    // is as close to "luftballons" as it is to "luftbal".
    let prefix: String = word.chars().take(query.chars().count()).collect();
    let distance = edit_distance(query, word).min(edit_distance(query, &prefix));
    match distance {
        1 => ONE_EDIT,
        2 if allowed >= 2 => TWO_EDITS,
        _ => 0,
    }
}

/// A query word's best score across a field's words, weighted.
fn field_score(query: &str, field: &[String], weight: u32) -> u32 {
    field
        .iter()
        .map(|w| word_score(query, w))
        .max()
        .unwrap_or(0)
        * weight
}

/// The searchable fields of one entry, prepared once per rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Haystack {
    /// Title words.
    pub title: Vec<String>,
    /// Artist words.
    pub artist: Vec<String>,
    /// Genre words (empty when untagged).
    pub genre: Vec<String>,
}

impl Haystack {
    /// Prepare an entry's fields.
    #[must_use]
    pub fn new(title: &str, artist: &str, genre: Option<&str>) -> Self {
        Haystack {
            title: words(title),
            artist: words(artist),
            genre: genre.map(words).unwrap_or_default(),
        }
    }

    /// The entry's score for a query (already split into words), or
    /// `None` when some query word finds nothing. An empty query
    /// matches everything at score 0.
    #[must_use]
    pub fn score(&self, query: &[String]) -> Option<u32> {
        let mut total = 0;
        for q in query {
            let best = field_score(q, &self.title, TITLE_WEIGHT)
                .max(field_score(q, &self.artist, ARTIST_WEIGHT))
                .max(field_score(q, &self.genre, GENRE_WEIGHT));
            if best == 0 {
                return None;
            }
            total += best;
        }
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> Vec<String> {
        words(text)
    }

    #[test]
    fn words_fold_case_diacritics_apostrophes_and_punctuation() {
        assert_eq!(q("Don't Stop Believin'"), ["dont", "stop", "believin"]);
        assert_eq!(
            q("Metallica- Nothing Else Matters"),
            ["metallica", "nothing", "else", "matters"]
        );
        assert_eq!(q("Ella, elle l'a"), ["ella", "elle", "la"]);
        assert_eq!(q("Sacré  Cœur"), ["sacre", "coeur"]);
        assert_eq!(
            q("Two of Hearts - Skatebård Remix"),
            ["two", "of", "hearts", "skatebard", "remix"]
        );
        assert_eq!(q("   "), Vec::<String>::new());
    }

    #[test]
    fn edit_distance_counts_each_kind_of_slip_as_one() {
        assert_eq!(edit_distance("smells", "smells"), 0);
        assert_eq!(edit_distance("smels", "smells"), 1, "missing letter");
        assert_eq!(edit_distance("smellls", "smells"), 1, "extra letter");
        assert_eq!(edit_distance("smalls", "smells"), 1, "wrong letter");
        assert_eq!(edit_distance("semlls", "smells"), 1, "swapped pair");
        assert_eq!(edit_distance("luftbalons", "luftballons"), 1);
        assert_eq!(edit_distance("abc", "xyz"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn a_word_scores_exact_over_prefix_over_inside_over_typo() {
        assert_eq!(word_score("life", "life"), EXACT);
        assert_eq!(word_score("lif", "life"), PREFIX);
        assert_eq!(word_score("ife", "life"), INSIDE);
        assert_eq!(word_score("smels", "smells"), ONE_EDIT);
        assert_eq!(word_score("luftbalons", "luftballons"), ONE_EDIT);
        assert_eq!(
            word_score("beleivin", "believin"),
            ONE_EDIT,
            "a swap is one edit"
        );
        assert_eq!(word_score("beleving", "believin"), TWO_EDITS);
        assert_eq!(
            word_score("luftbla", "luftballons"),
            ONE_EDIT,
            "typo mid-typing, against the prefix"
        );
        assert_eq!(word_score("xyz", "life"), 0);
    }

    #[test]
    fn short_words_get_no_slack_and_long_words_get_two_edits() {
        assert_eq!(allowed_edits(3), 0);
        assert_eq!(allowed_edits(4), 1);
        assert_eq!(allowed_edits(7), 1);
        assert_eq!(allowed_edits(8), 2);
        // "the" must not find "toe": with one edit a three-letter
        // word finds half the library. Four letters get one edit -
        // "armi" finds "army" - and pay for it with "like" being a
        // (low-ranked) hit for "life".
        assert_eq!(word_score("the", "toe"), 0);
        assert_eq!(word_score("armi", "army"), ONE_EDIT);
        assert_eq!(word_score("life", "like"), ONE_EDIT);
        // The first letter must agree: "gall" is not "all", "call"
        // or "sally", however close the edit distance says they are.
        assert_eq!(word_score("gall", "all"), 0);
        assert_eq!(word_score("gall", "callinan"), 0);
        assert_eq!(word_score("gall", "sally"), 0);
    }

    #[test]
    fn every_query_word_must_hit_and_the_score_adds_up() {
        let nirvana = Haystack::new("Smells Like Teen Spirit", "Nirvana", Some("Grunge"));
        assert_eq!(
            nirvana.score(&q("smels like")),
            Some(ONE_EDIT * 2 + EXACT * 2)
        );
        assert_eq!(
            nirvana.score(&q("nirvana spirit")),
            Some(EXACT * 2 + EXACT * 2)
        );
        assert_eq!(
            nirvana.score(&q("smells queen")),
            None,
            "AND: one miss sinks it"
        );
        assert_eq!(
            nirvana.score(&q("")),
            Some(0),
            "an empty query matches everything"
        );
        // Genre counts single: "grunge" scores less than the same
        // word would in the title.
        assert_eq!(nirvana.score(&q("grunge")), Some(EXACT));
    }

    #[test]
    fn the_real_titles_that_started_this() {
        let journey = Haystack::new("Don't Stop Believin'", "Journey", None);
        assert!(
            journey.score(&q("dont stop")).is_some(),
            "apostrophe is not a letter"
        );
        assert!(
            journey.score(&q("don't stop")).is_some(),
            "typed with it, too"
        );
        let nena = Haystack::new("99 Luftballons", "Nena", None);
        assert!(nena.score(&q("luftbalons")).is_some());
        assert!(nena.score(&q("99")).is_some());
        let metallica = Haystack::new("Metallica- Nothing Else Matters", "Metallica", None);
        assert!(metallica.score(&q("nothing else")).is_some());
        assert!(metallica.score(&q("metalica")).is_some(), "one l short");
        let gall = Haystack::new("Ella, elle l'a", "France Gall", Some("Chanson"));
        assert!(gall.score(&q("gall")).is_some(), "artist");
        assert!(gall.score(&q("ella elle")).is_some());
    }
}
