//! The one part that touches the network: a search, and a download.
//!
//! What leaves the machine is the artist and title the player typed and,
//! for the chosen chart, its checksum. The catalogue answers HTTP 500
//! now and then (seen throughout the bulk import), so a search is tried
//! a few times before it gives up.

use std::io::Read;
use std::time::Duration;

use crate::hit::{Hit, parse_search};
use crate::sng::MAX_PACKAGE;

/// The search endpoint that matches title AND artist on the server.
const ADVANCED: &str = "https://api.enchor.us/search/advanced";

/// The plain search, which matches the title only.
const PLAIN: &str = "https://api.enchor.us/search";

/// How long to wait for a search answer.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a download may take (a package with stems is tens of MB).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// How many times a search is tried.
const TRIES: u32 = 3;

/// The catalogue client.
pub struct Client {
    agent: ureq::Agent,
    user_agent: String,
}

/// The request body of the advanced search. Pure — tested.
#[must_use]
pub fn advanced_body(artist: &str, title: &str) -> serde_json::Value {
    let field =
        |value: &str| serde_json::json!({ "value": value, "exact": false, "exclude": false });
    serde_json::json!({
        "name": field(title),
        "artist": field(artist),
        "instrument": "guitar",
        "difficulty": "expert",
        "page": 1,
    })
}

/// The request body of the plain search. Pure — tested.
#[must_use]
pub fn plain_body(title: &str) -> serde_json::Value {
    serde_json::json!({
        "search": title,
        "page": 1,
        "instrument": "guitar",
        "difficulty": "expert",
        "drumType": null,
        "source": "bridge",
    })
}

impl Client {
    /// A client that names this version of BeatByte.
    #[must_use]
    pub fn new(version: &str) -> Client {
        Client {
            agent: ureq::AgentBuilder::new()
                .timeout_read(SEARCH_TIMEOUT)
                .timeout_write(SEARCH_TIMEOUT)
                .build(),
            user_agent: format!("BeatByte/{version} ( https://github.com/pepperonas/beatbyte )"),
        }
    }

    /// Every chart of this song the catalogue has: the advanced search
    /// first (title and artist on the server), the plain one when that
    /// finds nothing — an artist spelled another way is still the song,
    /// and [`crate::choose::candidates`] checks the artist again.
    ///
    /// # Errors
    /// When the catalogue cannot be reached or does not answer sensibly.
    pub fn search(&self, artist: &str, title: &str) -> Result<Vec<Hit>, String> {
        let hits = self.post(ADVANCED, &advanced_body(artist, title))?;
        if hits.is_empty() {
            self.post(PLAIN, &plain_body(title))
        } else {
            Ok(hits)
        }
    }

    fn post(&self, url: &str, body: &serde_json::Value) -> Result<Vec<Hit>, String> {
        let mut last = String::new();
        for attempt in 0..TRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(2 * u64::from(attempt)));
            }
            match self
                .agent
                .post(url)
                .set("User-Agent", &self.user_agent)
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())
            {
                Ok(response) => {
                    let text = response
                        .into_string()
                        .map_err(|e| format!("the catalogue's answer broke off: {e}"))?;
                    return parse_search(&text);
                }
                Err(ureq::Error::Status(code, _)) if code >= 500 => {
                    last = format!("the catalogue is busy (HTTP {code})");
                }
                Err(ureq::Error::Status(code, _)) => {
                    return Err(format!("the catalogue refused the search (HTTP {code})"));
                }
                Err(ureq::Error::Transport(e)) => {
                    return Err(format!("the catalogue cannot be reached: {e}"));
                }
            }
        }
        Err(last)
    }

    /// Download a chart's package, capped at [`MAX_PACKAGE`].
    ///
    /// # Errors
    /// When the hit has no valid checksum, or the download fails or is
    /// larger than any chart.
    pub fn download(&self, hit: &Hit) -> Result<Vec<u8>, String> {
        let url = hit
            .download_url()
            .ok_or("the catalogue named no valid package")?;
        let response = self
            .agent
            .get(&url)
            .timeout(DOWNLOAD_TIMEOUT)
            .set("User-Agent", &self.user_agent)
            .call()
            .map_err(|e| format!("the download failed: {e}"))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_PACKAGE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("the download broke off: {e}"))?;
        if bytes.len() > MAX_PACKAGE {
            return Err("the download is larger than any chart".to_owned());
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against the real catalogue: the search finds the song, the best
    /// candidate downloads, and the package decodes into a chart folder.
    /// Ignored by default — it needs the network and fetches megabytes;
    /// run with `cargo test -p beatbyte-encore -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn the_real_catalogue_answers_and_a_package_decodes() {
        let client = Client::new("test");
        let hits = client.search("3 Doors Down", "Kryptonite").expect("search");
        let wanted = crate::Wanted {
            artist: "3 Doors Down".to_owned(),
            title: "Kryptonite".to_owned(),
            length_s: None,
        };
        let best = crate::candidates(&hits, &wanted);
        assert!(!best.is_empty(), "{hits:?}");
        let bytes = client.download(best[0]).expect("download");
        let package = crate::decode(&bytes).expect("decode");
        assert!(package.has_chart());
        assert!(
            package
                .files
                .iter()
                .any(|(n, _)| n.ends_with(".ogg") || n.ends_with(".opus")),
            "audio in the package: {:?}",
            package.files.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
    }

    /// What leaves the machine: the title and the artist, guitar Expert,
    /// nothing else.
    #[test]
    fn a_search_sends_only_the_song() {
        let body = advanced_body("3 Doors Down", "Kryptonite");
        assert_eq!(body["name"]["value"], "Kryptonite");
        assert_eq!(body["artist"]["value"], "3 Doors Down");
        assert_eq!(body["instrument"], "guitar");
        let keys: Vec<&String> = body.as_object().expect("object").keys().collect();
        assert_eq!(keys.len(), 5, "{keys:?}");
        let plain = plain_body("Kryptonite");
        assert_eq!(plain["search"], "Kryptonite");
        assert_eq!(plain["source"], "bridge");
    }
}
