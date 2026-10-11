//! One search result, read from the catalogue's JSON.
//!
//! The answer is untrusted input: every field is optional, a string that
//! is not a string becomes empty, and an md5 that is not 32 hex digits
//! makes the hit unusable — that value ends up in a download URL.

use serde_json::Value;

/// A chart the catalogue knows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hit {
    /// The package's checksum; also its download name.
    pub md5: String,
    /// The song's title as the chart names it.
    pub name: String,
    /// The artist as the chart names it.
    pub artist: String,
    /// Who charted it (a studio for a game rip).
    pub charter: String,
    /// The pack it came in ("Rock Band 2 DLC", "Custom Songs Central").
    pub pack: String,
    /// Song length in milliseconds, 0 when unknown.
    pub length_ms: u64,
    /// A chart made for effect rather than to be played.
    pub modchart: bool,
    /// It carries a video background (larger download, nothing for us).
    pub video: bool,
    /// It uses open notes, which BeatByte has no lane for.
    pub opens: bool,
    /// How many problems the catalogue's own checker reported.
    pub issues: usize,
    /// Guitar notes per difficulty: (difficulty name, count).
    pub guitar: Vec<(String, u64)>,
}

impl Hit {
    /// Expert guitar notes, 0 when the chart has no Expert part.
    #[must_use]
    pub fn expert_notes(&self) -> u64 {
        self.notes_on("expert")
    }

    /// Guitar notes on one difficulty.
    #[must_use]
    pub fn notes_on(&self, difficulty: &str) -> u64 {
        self.guitar
            .iter()
            .find(|(d, _)| d == difficulty)
            .map_or(0, |(_, n)| *n)
    }

    /// The download URL. Only ever built from a checked md5.
    #[must_use]
    pub fn download_url(&self) -> Option<String> {
        is_md5(&self.md5).then(|| format!("https://files.enchor.us/{}.sng", self.md5))
    }
}

/// Whether `text` is an md5: exactly 32 lowercase hex digits. Pure —
/// tested.
#[must_use]
pub fn is_md5(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Read a search answer. Hits without a usable md5 are dropped; one md5
/// listed twice (the same package in two packs) is kept once.
///
/// # Errors
/// When the text is not JSON or has no `data` list.
pub fn parse_search(json: &str) -> Result<Vec<Hit>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("not a catalogue answer: {e}"))?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or("the catalogue answer has no results list")?;
    let mut hits: Vec<Hit> = Vec::new();
    for item in data {
        let hit = read_hit(item);
        if is_md5(&hit.md5) && !hits.iter().any(|h| h.md5 == hit.md5) {
            hits.push(hit);
        }
    }
    Ok(hits)
}

fn text(item: &Value, key: &str) -> String {
    item.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn read_hit(item: &Value) -> Hit {
    let notes = item.get("notesData");
    let guitar = notes
        .and_then(|n| n.get("noteCounts"))
        .and_then(Value::as_array)
        .map(|counts| {
            counts
                .iter()
                .filter(|c| c.get("instrument").and_then(Value::as_str) == Some("guitar"))
                .filter_map(|c| {
                    Some((
                        c.get("difficulty")?.as_str()?.to_owned(),
                        c.get("count")?.as_u64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Hit {
        md5: text(item, "md5"),
        name: text(item, "name"),
        artist: text(item, "artist"),
        charter: text(item, "charter"),
        pack: text(item, "packName"),
        length_ms: item.get("song_length").and_then(Value::as_u64).unwrap_or(0),
        modchart: item
            .get("modchart")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        video: item
            .get("hasVideoBackground")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        opens: notes
            .and_then(|n| n.get("hasOpenNotes"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        issues: notes
            .and_then(|n| n.get("chartIssues"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        guitar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KRYPTONITE: &str = include_str!("../tests/fixtures/kryptonite-advanced.json");
    const AFRICA: &str = include_str!("../tests/fixtures/africa-search.json");

    /// A real answer reads into hits with every field the choice needs.
    #[test]
    fn a_recorded_answer_reads_into_hits() {
        let hits = parse_search(KRYPTONITE).expect("parses");
        assert_eq!(hits.len(), 3);
        let angevil = hits
            .iter()
            .find(|h| h.charter == "Angevil")
            .expect("community chart");
        assert_eq!(angevil.artist, "3 Doors Down");
        assert_eq!(angevil.name, "Kryptonite");
        assert!(angevil.opens);
        assert!(angevil.expert_notes() > 100);
        let rb2 = hits.iter().find(|h| h.charter == "Harmonix").expect("rip");
        assert_eq!(rb2.pack, "Rock Band 2 DLC");
        assert!(!rb2.opens);
    }

    /// The same package listed under two packs is one hit.
    #[test]
    fn one_package_listed_twice_is_kept_once() {
        let hits = parse_search(AFRICA).expect("parses");
        let mut md5s: Vec<&str> = hits.iter().map(|h| h.md5.as_str()).collect();
        let before = md5s.len();
        md5s.sort_unstable();
        md5s.dedup();
        assert_eq!(md5s.len(), before);
        assert!(before < 12, "the fixture lists two packages twice");
    }

    /// An md5 that is not one never reaches a URL — it is the one value
    /// from the answer that becomes part of a request.
    #[test]
    fn only_a_real_md5_becomes_a_download() {
        assert!(is_md5("8284631a0123456789abcdef01234567"));
        for bad in [
            "",
            "8284631a",
            "../../etc/passwd000000000000000000",
            "8284631A0123456789ABCDEF01234567",
            "8284631a0123456789abcdef0123456g",
            "8284631a0123456789abcdef01234567/",
        ] {
            assert!(!is_md5(bad), "{bad:?}");
            let hit = Hit {
                md5: bad.to_owned(),
                ..Hit::default()
            };
            assert_eq!(hit.download_url(), None);
        }
        let json = r#"{"data":[{"md5":"../x","name":"A"},{"md5":"8284631a0123456789abcdef01234567","name":"B"}]}"#;
        let hits = parse_search(json).expect("parses");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "B");
    }

    /// Garbage is an error, never a panic; odd field types are empty.
    #[test]
    fn a_broken_answer_is_an_error_and_odd_fields_are_empty() {
        assert!(parse_search("not json").is_err());
        assert!(parse_search("{}").is_err());
        let json =
            r#"{"data":[{"md5":"8284631a0123456789abcdef01234567","name":7,"song_length":"x"}]}"#;
        let hits = parse_search(json).expect("parses");
        assert_eq!(hits[0].name, "");
        assert_eq!(hits[0].length_ms, 0);
    }
}
