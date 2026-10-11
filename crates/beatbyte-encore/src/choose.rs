//! Which of the catalogue's charts is the song the player asked for, and
//! in which order to try them. Pure — every rule is pinned.
//!
//! The rules come from the bulk import (`tools/bridge-bulk/catalog.py`,
//! more than eight thousand songs fetched with them):
//! - **the same song**: the title matches with its furniture removed
//!   (brackets, "The", punctuation), and the artist shares a name;
//! - **the original recording**: a cover, a live take, a remix, a
//!   parody … is left out — unless the player asked for exactly that;
//! - **playable**: an Expert guitar part of at least
//!   [`MIN_EXPERT_NOTES`] notes, no "modchart", a song length a song has;
//! - **best first**: a community chart before a game rip, then no video,
//!   no open notes (BeatByte has no open lane), fewest reported issues,
//!   most levels charted by hand, most Expert notes.

use crate::hit::Hit;

/// The fewest Expert notes a chart may have to count as a chart.
pub const MIN_EXPERT_NOTES: u64 = 100;

/// The shortest and longest song a chart may be for, in milliseconds.
pub const LENGTH_MS: (u64, u64) = (60_000, 600_000);

/// What the player asked for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wanted {
    /// The artist, as typed or as the catalogue named it.
    pub artist: String,
    /// The title.
    pub title: String,
    /// The recording's length in seconds, when something already knows
    /// it (the lyrics catalogue). A chart for a much longer or shorter
    /// recording is another edit.
    pub length_s: Option<f64>,
}

/// The charts that are this song, best first. Empty when none fits —
/// [`refusal`] then says why, in words for the status line.
#[must_use]
pub fn candidates<'a>(hits: &'a [Hit], wanted: &Wanted) -> Vec<&'a Hit> {
    let mut fitting: Vec<&Hit> = hits.iter().filter(|hit| fits(hit, wanted)).collect();
    fitting.sort_by_key(|hit| rank(hit));
    fitting
}

/// Why no chart fits, for the status line. Pure — tested.
#[must_use]
pub fn refusal(hits: &[Hit], wanted: &Wanted) -> String {
    if hits.is_empty() {
        return "no community chart of it".to_owned();
    }
    let same: Vec<&Hit> = hits.iter().filter(|h| same_song(h, wanted)).collect();
    if same.is_empty() {
        return "no community chart of this song".to_owned();
    }
    if same.iter().all(|h| !original(h, wanted)) {
        return "only covers, live takes or remixes of it".to_owned();
    }
    "no playable community chart of it".to_owned()
}

/// Whether a hit is the song, the original recording, and playable.
#[must_use]
pub fn fits(hit: &Hit, wanted: &Wanted) -> bool {
    same_song(hit, wanted) && original(hit, wanted) && playable(hit) && length_fits(hit, wanted)
}

/// The order key: smaller is better.
fn rank(
    hit: &Hit,
) -> (
    bool,
    bool,
    bool,
    usize,
    std::cmp::Reverse<usize>,
    std::cmp::Reverse<u64>,
) {
    (
        is_rip(hit),
        hit.video,
        hit.opens,
        hit.issues,
        std::cmp::Reverse(hit.guitar.len()),
        std::cmp::Reverse(hit.expert_notes()),
    )
}

/// Same title, and the artists share a name.
fn same_song(hit: &Hit, wanted: &Wanted) -> bool {
    let title = norm(&wanted.title);
    !title.is_empty()
        && squash(&norm(&hit.name)) == squash(&title)
        && artists_meet(&hit.artist, &wanted.artist)
}

/// Not a cover, live take, remix … — unless the player asked for one.
fn original(hit: &Hit, wanted: &Wanted) -> bool {
    let asked = words(&wanted.title);
    let junk = |text: &str| {
        let lower = text.to_lowercase();
        let text_words = words(text);
        JUNK_WORDS
            .iter()
            .any(|w| text_words.iter().any(|t| t == w) && !asked.iter().any(|a| a == w))
            || JUNK_PHRASES
                .iter()
                .any(|p| lower.contains(p) && !wanted.title.to_lowercase().contains(p))
    };
    !junk(&hit.name) && !junk(&hit.artist)
}

/// An Expert part worth the name, no modchart, a song's length.
fn playable(hit: &Hit) -> bool {
    !hit.modchart
        && hit.expert_notes() >= MIN_EXPERT_NOTES
        && (LENGTH_MS.0..=LENGTH_MS.1).contains(&hit.length_ms)
}

/// Within the larger of 20 s and 10 % of the known length, or no length
/// known on either side.
fn length_fits(hit: &Hit, wanted: &Wanted) -> bool {
    match wanted.length_s {
        Some(seconds) if hit.length_ms > 0 => {
            #[allow(clippy::cast_precision_loss)] // a song's milliseconds
            let chart = hit.length_ms as f64 / 1000.0;
            (chart - seconds).abs() <= (seconds * 0.10).max(20.0)
        }
        _ => true,
    }
}

/// Words that mark a hit as not the original recording, or not a song.
const JUNK_WORDS: [&str; 25] = [
    "live",
    "remix",
    "cover",
    "karaoke",
    "instrumental",
    "acoustic",
    "demo",
    "medley",
    "mashup",
    "parody",
    "nightcore",
    "meme",
    "troll",
    "joke",
    "version",
    "edit",
    "mix",
    "reprise",
    "intro",
    "outro",
    "vs",
    "slowed",
    "8bit",
    "remaster",
    "rerecorded",
];

/// Phrases that do the same.
const JUNK_PHRASES: [&str; 6] = ["full album", "sung by", "but the", "sped up", "(feat", "["];

/// The studios that charted the commercial games.
const STUDIOS: [&str; 10] = [
    "harmonix",
    "neversoft",
    "vicarious visions",
    "redoctane",
    "budcat",
    "freestylegames",
    "wavegroup",
    "activision",
    "beenox",
    "ubisoft",
];

/// A chart of a commercial game: made by one of its studios, or in a
/// pack that IS one of the games. A community pack with "Rock Band" in
/// its name is not ("J-Rock Band Project"), nor a fan game ("Guitar Hero
/// X-II"). Pure — tested.
#[must_use]
pub fn is_rip(hit: &Hit) -> bool {
    let charter = format!(" {} ", words(&hit.charter).join(" "));
    STUDIOS
        .iter()
        .any(|studio| charter.contains(&format!(" {studio} ")))
        || is_game_pack(&hit.pack)
}

/// The game a rip comes from, for the version's heading: its pack when
/// the pack IS a game ("Rock Band 3 DLC"), else the studio among its
/// charters, else its pack. `None` for a community chart. Pure — tested.
#[must_use]
pub fn game_of(hit: &Hit) -> Option<String> {
    if !is_rip(hit) {
        return None;
    }
    if is_game_pack(&hit.pack) {
        return Some(hit.pack.trim().to_owned());
    }
    let charter = format!(" {} ", words(&hit.charter).join(" "));
    let studio = STUDIOS
        .iter()
        .find(|studio| charter.contains(&format!(" {studio} ")));
    Some(match studio {
        Some(studio) => title_case(studio),
        None if !hit.pack.trim().is_empty() => hit.pack.trim().to_owned(),
        None => "a game".to_owned(),
    })
}

fn title_case(text: &str) -> String {
    text.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The share of a chart's notes that may be open notes before the chart
/// is not worth importing: BeatByte has no open lane, so they are
/// dropped, and a chart that loses more than this plays with holes.
pub const MAX_OPEN_SHARE: f64 = 0.05;

/// Whether a converted chart lost too many open notes (the bulk import
/// rejected those by hand; Thunderstruck lost 499). Pure — tested.
#[must_use]
pub fn too_many_opens(dropped: usize, kept_expert_notes: usize) -> bool {
    let total = dropped + kept_expert_notes;
    #[allow(clippy::cast_precision_loss)] // note counts
    let share = if total == 0 {
        0.0
    } else {
        dropped as f64 / total as f64
    };
    share > MAX_OPEN_SHARE
}

/// Whether a pack name is a commercial game or its DLC. Pure — tested.
#[must_use]
pub fn is_game_pack(pack: &str) -> bool {
    let pack = pack.trim().to_lowercase();
    let mut rest = pack.as_str();
    for prefix in [
        "the beatles: ",
        "the beatles ",
        "green day: ",
        "green day ",
        "lego ",
        "ac/dc ",
    ] {
        if let Some(after) = rest.strip_prefix(prefix) {
            rest = after;
            break;
        }
    }
    const GAMES: [(&str, &[&str]); 6] = [
        (
            "rock band",
            &[
                " 1",
                " 2",
                " 3",
                " 4",
                " network",
                " blitz",
                " unplugged",
                " rivals",
                " vr",
            ],
        ),
        (
            "guitar hero",
            &[
                " ii",
                " iii",
                " 2",
                " 3",
                " 5",
                " world tour",
                " smash hits",
                " metallica",
                " aerosmith",
                " van halen",
                " warriors of rock",
                " on tour",
                " encore",
                " 80s",
                " live",
                " greatest hits",
            ],
        ),
        ("band hero", &[]),
        ("dj hero", &[" 2"]),
        ("rocksmith", &[" 2014"]),
        ("guitar hero", &[]),
    ];
    for (game, editions) in GAMES {
        let Some(after) = rest.strip_prefix(game) else {
            continue;
        };
        // The longest edition that fits, then nothing at all.
        let mut tails: Vec<&str> = editions
            .iter()
            .filter_map(|e| after.strip_prefix(e))
            .collect();
        tails.push(after);
        if tails.into_iter().any(game_pack_ends) {
            return true;
        }
    }
    false
}

/// What may follow a game's name: nothing, "DLC", or a subtitle after
/// `:`, `(` or `-`.
fn game_pack_ends(tail: &str) -> bool {
    let tail = tail.trim_start();
    tail.is_empty()
        || tail == "dlc"
        || tail.starts_with("dlc ")
        || tail.starts_with([':', '(', '-'])
}

/// A title or artist reduced to what identifies it: lowercase, without
/// rich-text tags, bracketed furniture, punctuation or a leading "The".
/// Pure — tested.
#[must_use]
pub fn norm(text: &str) -> String {
    let mut out = String::new();
    let mut depth_tag = 0usize;
    let mut depth_round = 0usize;
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            '<' => depth_tag += 1,
            '>' => depth_tag = depth_tag.saturating_sub(1),
            '(' => depth_round += 1,
            ')' => depth_round = depth_round.saturating_sub(1),
            _ if depth_tag > 0 || depth_round > 0 => {}
            c if c.is_alphanumeric() => out.push(fold(c)),
            _ => out.push(' '),
        }
    }
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    joined
        .strip_prefix("the ")
        .map_or(joined.clone(), str::to_owned)
}

/// The same with the spaces gone too — "Hand In Glove" and "Hand-in
/// Glove" are one title.
fn squash(normed: &str) -> String {
    normed.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The common accented letters, folded the way a search would.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// Lowercase words, punctuation as separators.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Each credited artist on their own, and all of them together.
fn artist_keys(artist: &str) -> Vec<String> {
    let normed = norm(artist);
    let mut keys: Vec<String> = vec![squash(&normed)];
    let mut part: Vec<&str> = Vec::new();
    let separators = ["feat", "ft", "featuring", "and", "x", "with"];
    for word in normed.split_whitespace().chain(std::iter::once("and")) {
        if separators.contains(&word) {
            if !part.is_empty() {
                keys.push(part.concat());
                part.clear();
            }
        } else {
            part.push(word);
        }
    }
    keys.retain(|k| !k.is_empty());
    keys
}

/// Whether two artist credits share a name.
fn artists_meet(a: &str, b: &str) -> bool {
    let a = artist_keys(a);
    let b = artist_keys(b);
    a.iter().any(|k| b.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit::parse_search;

    const KRYPTONITE: &str = include_str!("../tests/fixtures/kryptonite-advanced.json");
    const AFRICA: &str = include_str!("../tests/fixtures/africa-search.json");

    fn wanted(artist: &str, title: &str) -> Wanted {
        Wanted {
            artist: artist.to_owned(),
            title: title.to_owned(),
            length_s: None,
        }
    }

    fn hit(name: &str, artist: &str, charter: &str, pack: &str) -> Hit {
        Hit {
            md5: "0123456789abcdef0123456789abcdef".to_owned(),
            name: name.to_owned(),
            artist: artist.to_owned(),
            charter: charter.to_owned(),
            pack: pack.to_owned(),
            length_ms: 240_000,
            guitar: vec![("expert".to_owned(), 900), ("hard".to_owned(), 700)],
            ..Hit::default()
        }
    }

    /// On the real Kryptonite answer the community chart comes first,
    /// then the two game rips — a community chart is preferred even with
    /// open notes, which is the bulk import's rule.
    #[test]
    fn the_community_chart_comes_before_the_rips() {
        let hits = parse_search(KRYPTONITE).expect("parses");
        let order: Vec<&str> = candidates(&hits, &wanted("3 Doors Down", "Kryptonite"))
            .iter()
            .map(|h| h.charter.as_str())
            .collect();
        assert_eq!(order.first(), Some(&"Angevil"), "{order:?}");
        assert_eq!(order.len(), 3, "both rips stay as fallbacks: {order:?}");
        // Among the rips the one without open notes goes first.
        assert_eq!(order[1], "Harmonix", "{order:?}");
    }

    /// The real Africa answer holds only covers and other songs: none
    /// fits Toto's, and the refusal says so in words.
    #[test]
    fn only_covers_fit_nothing_and_say_so() {
        let hits = parse_search(AFRICA).expect("parses");
        let want = wanted("Toto", "Africa");
        assert!(candidates(&hits, &want).is_empty());
        assert_eq!(refusal(&hits, &want), "no community chart of this song");
        assert_eq!(refusal(&[], &want), "no community chart of it");
        // Asked for the cover's artist, it is a cover — and refused as one.
        let covers = wanted("Leo Moracchioli", "Africa");
        assert!(candidates(&hits, &covers).is_empty());
        assert_eq!(
            refusal(&hits, &covers),
            "only covers, live takes or remixes of it"
        );
    }

    /// The artist decides between two songs of one name.
    #[test]
    fn the_artist_decides_between_two_songs_of_one_name() {
        let hits = [
            hit("Kryptonite", "Grand", "Naonemeu", ""),
            hit("Kryptonite", "3 Doors Down", "Angevil", ""),
        ];
        let chosen = candidates(&hits, &wanted("3 doors down", "kryptonite"));
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].artist, "3 Doors Down");
        // A featured artist still counts.
        let feat = [hit("Stay", "Rihanna feat. Mikky Ekko", "X", "")];
        assert_eq!(candidates(&feat, &wanted("Rihanna", "Stay")).len(), 1);
        // No artist given: nothing can be told apart, nothing is taken.
        assert!(candidates(&hits, &wanted("", "Kryptonite")).is_empty());
    }

    /// The title's furniture does not matter; a different title does.
    #[test]
    fn a_title_matches_without_its_furniture() {
        let h = [hit(
            "The Final Countdown (Remastered 2009)",
            "Europe",
            "X",
            "",
        )];
        // "(Remastered …)" sits in brackets and goes with them.
        assert_eq!(
            candidates(&h, &wanted("Europe", "Final Countdown")).len(),
            1
        );
        let h = [hit("Hand-in Glove", "The Smiths", "X", "")];
        assert_eq!(candidates(&h, &wanted("Smiths", "Hand In Glove")).len(), 1);
        let h = [hit("Countdown", "Europe", "X", "")];
        assert!(candidates(&h, &wanted("Europe", "Final Countdown")).is_empty());
    }

    /// A junk word refuses — unless the player asked for it.
    #[test]
    fn a_live_take_is_refused_unless_asked_for() {
        let h = [hit("Live and Let Die", "Wings", "X", "")];
        assert_eq!(
            candidates(&h, &wanted("Wings", "Live and Let Die")).len(),
            1,
            "\"Live\" in the asked title is the title"
        );
        let h = [hit("Kryptonite - Live", "3 Doors Down", "X", "")];
        assert!(candidates(&h, &wanted("3 Doors Down", "Kryptonite")).is_empty());
    }

    /// Playable: an Expert part, no modchart, a song's length — and, when
    /// the recording's length is known, close to it.
    #[test]
    fn only_a_playable_chart_of_this_length_fits() {
        let good = hit("Song", "Band", "X", "");
        let want = wanted("Band", "Song");
        assert!(fits(&good, &want));
        let thin = Hit {
            guitar: vec![("expert".to_owned(), 99)],
            ..good.clone()
        };
        assert!(!fits(&thin, &want), "under 100 Expert notes");
        let no_expert = Hit {
            guitar: vec![("hard".to_owned(), 900)],
            ..good.clone()
        };
        assert!(!fits(&no_expert, &want));
        let mod_chart = Hit {
            modchart: true,
            ..good.clone()
        };
        assert!(!fits(&mod_chart, &want));
        let short = Hit {
            length_ms: 59_000,
            ..good.clone()
        };
        assert!(!fits(&short, &want));
        let timed = Wanted {
            length_s: Some(240.0),
            ..want.clone()
        };
        assert!(fits(&good, &timed));
        let long_edit = Hit {
            length_ms: 300_000,
            ..good.clone()
        };
        assert!(!fits(&long_edit, &timed), "an edit of another length");
        let near = Hit {
            length_ms: 258_000,
            ..good
        };
        assert!(fits(&near, &timed), "within 10 % / 20 s");
    }

    /// The order: community before rip, then no video, no opens, fewer
    /// issues, more levels, more notes.
    #[test]
    fn the_order_prefers_what_plays_best() {
        let base = hit("Song", "Band", "Someone", "");
        let rip = Hit {
            charter: "Harmonix".to_owned(),
            ..base.clone()
        };
        let opens = Hit {
            opens: true,
            ..base.clone()
        };
        let video = Hit {
            video: true,
            ..base.clone()
        };
        let fewer_levels = Hit {
            guitar: vec![("expert".to_owned(), 900)],
            ..base.clone()
        };
        let hits = [
            rip.clone(),
            opens.clone(),
            video.clone(),
            fewer_levels.clone(),
            base.clone(),
        ];
        let order = candidates(&hits, &wanted("Band", "Song"));
        assert_eq!(*order[0], base);
        assert_eq!(*order[1], fewer_levels);
        assert_eq!(*order[2], opens);
        assert_eq!(*order[3], video);
        assert_eq!(*order[4], rip, "a rip only when no community chart is left");
    }

    /// The rip rule: a studio among the charters, or a pack that IS a
    /// game — and not a community pack that merely names one.
    #[test]
    fn a_rip_is_a_studio_or_a_game_pack_and_nothing_else() {
        let by = |charter: &str, pack: &str| is_rip(&hit("S", "B", charter, pack));
        assert!(by("Harmonix", ""));
        assert!(by("ataeaf, FreeStyleGames", ""));
        assert!(by("Vicarious Visions", ""));
        assert!(!by("Harmonixfan", ""), "a word, not a substring");
        for pack in [
            "Rock Band 2 DLC",
            "Rock Band 3",
            "Guitar Hero 5",
            "Guitar Hero World Tour",
            "Guitar Hero: Smash Hits",
            "The Beatles: Rock Band",
            "Lego Rock Band",
            "Green Day: Rock Band",
            "Rocksmith 2014",
            "Rock Band Network",
            "Band Hero",
            "DJ Hero 2",
        ] {
            assert!(is_game_pack(pack), "{pack}");
        }
        for pack in [
            "J-Rock Band Project",
            "Guitar Hero X-II",
            "Custom Songs Central",
            "Rock Band Fans Setlist",
            "",
        ] {
            assert!(!is_game_pack(pack), "{pack}");
        }
    }

    /// A rip names its game: the pack when it is one, else the studio.
    #[test]
    fn a_rip_names_its_game_and_a_community_chart_none() {
        assert_eq!(
            game_of(&hit("S", "B", "Harmonix", "Rock Band 2 DLC")).as_deref(),
            Some("Rock Band 2 DLC")
        );
        assert_eq!(
            game_of(&hit("S", "B", "ataeaf, FreeStyleGames", "Custom Pack")).as_deref(),
            Some("Freestylegames")
        );
        assert_eq!(game_of(&hit("S", "B", "Angevil", "")), None);
    }

    /// Open notes: five per cent lost is the limit, measured against
    /// the chart as it was.
    #[test]
    fn a_chart_that_loses_too_many_open_notes_is_refused() {
        assert!(!too_many_opens(0, 900));
        assert!(!too_many_opens(50, 950), "exactly 5 %");
        assert!(too_many_opens(51, 949));
        assert!(too_many_opens(499, 700), "Thunderstruck");
        assert!(!too_many_opens(0, 0));
    }

    #[test]
    fn norm_strips_the_furniture() {
        assert_eq!(norm("The Final Countdown"), "final countdown");
        assert_eq!(norm("Hand-in Glove"), "hand in glove");
        assert_eq!(norm("<color=red>Africa</color> (Live)"), "africa");
        assert_eq!(norm("Blue Öyster Cult"), "blue oyster cult");
        assert_eq!(norm(""), "");
    }
}
