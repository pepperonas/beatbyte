//! The one part that touches the network.
//!
//! Everything about it is a promise kept to the service: a
//! descriptive `User-Agent` naming the application and a way to
//! reach its author, and **at most one request a second**, which
//! MusicBrainz asks for and which this enforces itself rather than
//! trusting a caller's loop to be polite.

use std::time::{Duration, Instant};

use crate::pick::{Recording, parse};

/// The shortest gap between two requests.
///
/// MusicBrainz asks for no more than one a second and will start
/// refusing above it. Enforced inside the client so a caller cannot
/// forget: the rate limit is the service's rule, not the caller's
/// preference.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// How long to wait for an answer before giving up on it.
const TIMEOUT: Duration = Duration::from_secs(15);

/// How many times to wait out a "busy" before giving up.
const RETRIES: u32 = 2;

/// How long to wait after the first refusal; the second waits twice
/// as long.
const BACKOFF: Duration = Duration::from_secs(3);

/// Whether an error means "ask again later" rather than "no".
///
/// Pure, so the policy can be pinned: 503 is the service asking for
/// room, and 429 is it saying so outright.
#[must_use]
pub fn busy_status(status: u16) -> bool {
    matches!(status, 429 | 503)
}

/// Whether this request should be tried again.
fn busy(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::Status(code, _) => busy_status(*code),
        // A transport error is a network that is not there. Waiting
        // three seconds will not conjure one.
        ureq::Error::Transport(_) => false,
    }
}

/// The `User-Agent` the service requires.
///
/// They ask for the application, its version and a way to reach
/// somebody. An anonymous agent is blocked, and rightly.
#[must_use]
pub fn user_agent(version: &str) -> String {
    format!("BeatByte/{version} ( https://github.com/pepperonas/beatbyte )")
}

/// A rate-limited connection to the catalogue.
pub struct Client {
    agent: ureq::Agent,
    user_agent: String,
    last: Option<Instant>,
}

impl Client {
    /// A client that identifies itself as this version of BeatByte.
    #[must_use]
    pub fn new(version: &str) -> Client {
        Client {
            agent: ureq::AgentBuilder::new()
                .timeout_read(TIMEOUT)
                .timeout_write(TIMEOUT)
                .build(),
            user_agent: user_agent(version),
            last: None,
        }
    }

    /// Wait out the rest of the interval, if any is left.
    fn wait_turn(&mut self) {
        if let Some(last) = self.last {
            let since = last.elapsed();
            if since < MIN_INTERVAL {
                std::thread::sleep(MIN_INTERVAL - since);
            }
        }
        self.last = Some(Instant::now());
    }

    /// Ask the catalogue about one song.
    ///
    /// What leaves the machine is an artist and a title, and nothing
    /// else. An error is a string the caller can print: a lookup
    /// that fails is a song that keeps its gaps, never a failure of
    /// anything else.
    pub fn search(&mut self, artist: &str, title: &str) -> Result<Vec<Recording>, String> {
        let (artist, title) = beatbyte_chart::catalogue::clean_query(artist, title);
        if artist.trim().is_empty() || title.trim().is_empty() {
            return Err("nothing to ask about".to_owned());
        }
        self.wait_turn();
        let query = format!(
            "artist:{} AND recording:{}",
            lucene(&artist),
            lucene(&title)
        );
        // ⚠️ One interval is not always enough. The service allows
        // an average of a request a second and still answers 503
        // when a run of heavy queries arrives back to back — four of
        // eighty-six were refused that way, measured. A refusal is a
        // "wait", so it is waited out once rather than treated as an
        // answer.
        let mut last = String::new();
        for attempt in 0..=RETRIES {
            if attempt > 0 {
                std::thread::sleep(BACKOFF * attempt);
                self.last = Some(Instant::now());
            }
            let result = self
                .agent
                .get("https://musicbrainz.org/ws/2/recording")
                .set("User-Agent", &self.user_agent)
                .query("query", &query)
                .query("fmt", "json")
                // A hundred, not a dozen. "Born to Run" has 2338
                // recordings and the first twelve are all live; the
                // studio take is not in a small page at all.
                .query("limit", "100")
                .call();
            match result {
                Ok(response) => {
                    return response
                        .into_string()
                        .map(|body| parse(&body))
                        .map_err(|error| format!("cannot read the answer: {error}"));
                }
                Err(error) => {
                    let again = busy(&error);
                    last = error.to_string();
                    if !again {
                        break;
                    }
                }
            }
        }
        Err(last)
    }
}

/// Quote a value for the catalogue's query language.
///
/// The field values are user data — a song title from a download —
/// and Lucene reads a dozen characters as syntax. Quoting the whole
/// value and escaping the quote is what keeps `AC/DC` a name rather
/// than an expression. Pure — tested.
#[must_use]
pub fn lucene(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_says_who_is_asking_and_how_to_reach_them() {
        // The service blocks an anonymous agent, and is right to.
        let agent = user_agent("0.18.11");
        assert!(agent.starts_with("BeatByte/0.18.11 ("));
        assert!(agent.contains("github.com/pepperonas/beatbyte"));
    }

    #[test]
    fn a_title_cannot_become_a_query_expression() {
        // A song title is user data. Unquoted, `AC/DC` and a title
        // with a colon in it are read as syntax.
        assert_eq!(lucene("AC/DC"), "\"AC/DC\"");
        assert_eq!(lucene("Hello: World"), "\"Hello: World\"");
        assert_eq!(lucene("say \"yes\""), "\"say \\\"yes\\\"\"");
        assert_eq!(lucene("back\\slash"), "\"back\\\\slash\"");
    }

    #[test]
    fn a_busy_service_is_asked_again_and_a_refusal_is_not() {
        // 503 is "make room", 429 is it said outright. A 404 is an
        // answer, and asking again would only spend somebody's rate
        // limit to hear it twice.
        assert!(busy_status(503) && busy_status(429));
        assert!(!busy_status(404) && !busy_status(400) && !busy_status(200));
    }

    #[test]
    fn the_rate_limit_is_the_services_rule_not_a_suggestion() {
        // A caller cannot opt out of it: the wait is inside the
        // client, before the request.
        assert!(MIN_INTERVAL >= Duration::from_secs(1));
        let mut client = Client::new("0.0.0");
        client.last = Some(Instant::now());
        let before = Instant::now();
        client.wait_turn();
        assert!(before.elapsed() >= Duration::from_secs(1), "it waited");
    }
}
