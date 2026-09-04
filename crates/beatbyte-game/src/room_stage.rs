//! Room Stage: the venue leaves the screen (roadmap G38, vision doc
//! `docs/room-stage.md`).
//!
//! Optional, **off by default**. When it is on, the game's own
//! ground-truth events — the note you hit, the judgment you earned,
//! the sustain you are holding, the Hype you fired — are posted to
//! tiny HTTP light services on the local network, so real lamps
//! answer the playing rather than guessing at the music through a
//! microphone.
//!
//! Three rules hold this apart from the game:
//!
//! - **Presentation only.** Nothing here can reach judgment. The
//!   listener reads messages that presentation consumers already
//!   read; the autopilot must score identically with the bridge on
//!   and off, and it is checked that way.
//! - **Drop, never block.** The frame thread pushes into a BOUNDED
//!   channel and moves on. A full queue drops the event; a dead
//!   endpoint, a wrong URL and a sleeping lamp all cost the game
//!   nothing but a dropped message.
//! - **Nothing leaves unasked.** Off by default, one address in the
//!   settings file, outbound only, no cloud and no account.
//!
//! The wire contract is the reference rig's (`lichtwerk-controller`,
//! `docs/room-stage.md`): `warn_kick` per hit, `warn_bass` while a
//! sustain burns, `warn_event` for accents, `effect` to engage and
//! release the scene. Any service that answers those four paths can
//! stand in — the mapping is pure and the posts are plain JSON.

use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use beatbyte_core::{Judgment, Lane, LaneSet, SessionEvent};
use bevy::prelude::*;

/// How many events may wait for the worker before the next one is
/// dropped.
///
/// Small on purpose: a backlog is not worth having. If the network
/// cannot keep up with the playing, the *newest* light cue is the
/// only interesting one, and a queue that grows would only deliver
/// yesterday's beat late.
pub const QUEUE: usize = 32;

/// How long a post may take before the worker gives up on it.
pub const TIMEOUT_MS: u64 = 250;

/// One post to a light service: a path under the configured base
/// address and a JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// The path, with its leading slash.
    pub path: &'static str,
    /// The JSON body, already rendered.
    pub body: String,
}

impl Post {
    fn new(path: &'static str, body: String) -> Post {
        Post { path, body }
    }
}

/// How hard a hit hits the room, by the judgment it earned.
///
/// This is the thing a microphone can never know: the strength comes
/// from *how well the note was played*, not from how loud the song
/// happens to be there.
#[must_use]
pub fn kick_strength(judgment: Judgment) -> f32 {
    match judgment {
        Judgment::Perfect => 1.0,
        Judgment::Great => 0.8,
        Judgment::Good => 0.6,
        // A miss is not a kick; it is the dip below.
        Judgment::Miss => 0.0,
    }
}

/// The colour hint for a note, low to high across the neck: green 0.0
/// to orange 1.0. A chord averages its lanes, so a two-note shape
/// lands between them rather than picking a winner.
///
/// The receiving contract calls this `tone` and reads it as a
/// treble share, which is exactly what the lane order means here.
#[must_use]
pub fn tone_of(lanes: LaneSet) -> f32 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for lane in Lane::ALL {
        if lanes.contains(lane) {
            sum += lane.index() as f32 / 4.0;
            count += 1.0;
        }
    }
    if count == 0.0 { 0.5 } else { sum / count }
}

/// What one session event asks the room to do.
///
/// `lanes` is the note's shape where the event has one (the session
/// event carries only an index; the listener resolves it against the
/// track). `bpm` rides along on every kick so the strip can lock its
/// tempo without guessing.
///
/// `None` for the events the room has nothing to say about — the
/// room is not a second scoreboard.
#[must_use]
pub fn post_for(event: SessionEvent, lanes: LaneSet, bpm: f64) -> Option<Post> {
    match event {
        SessionEvent::NoteHit { judgment, .. } => {
            let strength = kick_strength(judgment);
            Some(Post::new(
                "/api/warn_kick",
                format!(
                    "{{\"strength\":{:.3},\"bpm\":{:.2},\"vel\":{:.3},\"tone\":{:.3}}}",
                    strength,
                    bpm,
                    strength,
                    tone_of(lanes)
                ),
            ))
        }
        // The room flinches with the player: a miss and an overstrum
        // both drop the pressure wave to nothing.
        SessionEvent::NoteMissed { .. } | SessionEvent::Overstrum => {
            Some(Post::new("/api/warn_bass", "{\"level\":0.000}".to_owned()))
        }
        // A held sustain is a held beam.
        SessionEvent::SustainStarted { .. } => {
            Some(Post::new("/api/warn_bass", "{\"level\":1.000}".to_owned()))
        }
        SessionEvent::SustainEnded { .. } => {
            Some(Post::new("/api/warn_bass", "{\"level\":0.000}".to_owned()))
        }
        // Hype is the big one, and the contract's biggest accent.
        SessionEvent::HypeActivated => Some(Post::new(
            "/api/warn_event",
            "{\"kind\":\"meteor\"}".to_owned(),
        )),
        SessionEvent::HypeEnded => Some(Post::new(
            "/api/warn_event",
            "{\"kind\":\"shimmer\"}".to_owned(),
        )),
        // A completed phrase earned something: let the room say so.
        SessionEvent::PhraseCompleted { .. } => Some(Post::new(
            "/api/warn_event",
            "{\"kind\":\"burst\"}".to_owned(),
        )),
        // Nothing to light: a broken phrase is already a miss, and
        // failing is the song ending, which releases the scene.
        SessionEvent::PhraseBroken { .. } | SessionEvent::Failed => None,
    }
}

/// The post that engages the scene when a song starts, and the one
/// that releases it when the song is over.
#[must_use]
pub fn scene_post(engage: bool) -> Post {
    if engage {
        Post::new("/api/effect", "{\"effect\":\"iris_warn\"}".to_owned())
    } else {
        Post::new("/api/effect", "{\"effect\":\"off\"}".to_owned())
    }
}

/// Add a post to a batch, replacing an earlier one on the same path.
///
/// The room only ever wants the LATEST state of a path: the newest
/// kick, the current bass level, the scene's final say. An accent
/// (`warn_event`) is the exception — each one is its own gesture, and
/// swallowing a meteor into a shimmer would lose the moment the
/// player earned — so accents queue up rather than replacing.
///
/// Pure — tested.
pub fn push_coalesced(batch: &mut Vec<Post>, post: Post) {
    if post.path == "/api/warn_event" {
        batch.push(post);
        return;
    }
    if let Some(slot) = batch.iter_mut().find(|held| held.path == post.path) {
        *slot = post;
    } else {
        batch.push(post);
    }
}

/// The handle the game holds: a bounded queue into a worker thread.
///
/// Cloning is not needed — one lives in a resource — but the handle
/// is deliberately cheap and `Send`, mirroring `MusicHandle`.
#[derive(Resource)]
pub struct RoomStage {
    posts: Option<SyncSender<Post>>,
    /// How many events were dropped because the worker was behind.
    /// Read by the debug overlay and the tests; never by gameplay.
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// How many were handed over.
    sent: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl RoomStage {
    /// A handle that goes nowhere: what the game holds while Room
    /// Stage is off, and what every test that does not want a
    /// network uses.
    #[must_use]
    pub fn idle() -> RoomStage {
        RoomStage {
            posts: None,
            dropped: std::sync::Arc::default(),
            sent: std::sync::Arc::default(),
        }
    }

    /// Start a worker posting to `base` (for example
    /// `http://127.0.0.1:5006`).
    ///
    /// The thread owns the HTTP client and the failures: a post that
    /// cannot be delivered is dropped without a word, because the one
    /// thing this feature may never do is interrupt a song.
    #[must_use]
    pub fn connect(base: &str) -> RoomStage {
        let (posts, inbox) = sync_channel::<Post>(QUEUE);
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let base = base.trim_end_matches('/').to_owned();
        let worker_sent = std::sync::Arc::clone(&sent);
        std::thread::Builder::new()
            .name("room-stage".to_owned())
            .spawn(move || {
                let timeout = std::time::Duration::from_millis(TIMEOUT_MS);
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(timeout)
                    .timeout(timeout)
                    .build();
                while let Ok(first) = inbox.recv() {
                    // Whatever piled up while the last post was in
                    // flight is drained here and COALESCED: a slow
                    // light service should get the newest cue per
                    // path, not a backlog of stale ones. With a
                    // service that keeps up this loop finds nothing
                    // and the batch is the single post.
                    let mut batch = vec![first];
                    while let Ok(next) = inbox.try_recv() {
                        push_coalesced(&mut batch, next);
                    }
                    for post in batch {
                        let url = format!("{base}{}", post.path);
                        // Fire and forget — but the reply MUST be
                        // read to the end. An unread response leaves
                        // the pooled keep-alive connection with bytes
                        // still in it, and the NEXT post writes into
                        // that desync and hangs until its timeout.
                        // Measured: with the reply dropped, a song
                        // delivered 6 posts and threw away 111; with
                        // it consumed, the same song delivers all of
                        // them.
                        if let Ok(reply) = agent
                            .post(&url)
                            .set("Content-Type", "application/json")
                            .send_string(&post.body)
                        {
                            let _ = reply.into_string();
                        }
                        worker_sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            })
            .ok();
        RoomStage {
            posts: Some(posts),
            dropped,
            sent,
        }
    }

    /// Hand a post to the worker, or drop it. Never blocks, never
    /// fails loudly — the return value is for tests and the overlay.
    pub fn send(&self, post: Post) -> bool {
        let Some(posts) = &self.posts else {
            return false;
        };
        match posts.try_send(post) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.dropped
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                false
            }
        }
    }

    /// Whether a worker is running at all.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.posts.is_some()
    }

    /// How many posts were dropped for want of room in the queue.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many posts the worker has put on the wire.
    #[must_use]
    pub fn delivered(&self) -> usize {
        self.sent.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for RoomStage {
    fn default() -> Self {
        RoomStage::idle()
    }
}

/// Keep the worker in step with the setting: connect when Room
/// Stage is switched on (and configured), hang up when it goes off.
///
/// Dropping the handle drops the sender, which ends the worker
/// thread — so switching off really does stop talking to the room,
/// rather than leaving a thread holding a socket.
pub fn sync_room_stage(settings: Res<crate::config::Settings>, mut stage: ResMut<RoomStage>) {
    if !settings.is_changed() && stage.is_live() == wanted(&settings) {
        return;
    }
    match (wanted(&settings), stage.is_live()) {
        (true, false) => {
            info!("room stage: connecting to {}", settings.room_stage_url);
            *stage = RoomStage::connect(&settings.room_stage_url);
        }
        (false, true) => {
            info!("room stage: off");
            *stage = RoomStage::idle();
        }
        _ => {}
    }
}

/// Whether the settings ask for a live bridge at all.
#[must_use]
pub fn wanted(settings: &crate::config::Settings) -> bool {
    settings.room_lights && !settings.room_stage_url.trim().is_empty()
}

/// Engage the scene as a song starts, release it as it ends.
pub fn engage_room(stage: Res<RoomStage>) {
    stage.send(scene_post(true));
}

/// Release it. Also runs when the player quits mid-song: a room left
/// burning after the music stopped is the one failure mode a light
/// bridge must not have.
pub fn release_room(stage: Res<RoomStage>) {
    stage.send(scene_post(false));
    if stage.is_live() {
        // The timing budget, measured rather than assumed (vision
        // doc TODO 6): how much of the playing actually reached the
        // room, and how much the queue had to throw away.
        info!(
            "room stage: {} posts delivered, {} dropped",
            stage.delivered(),
            stage.dropped()
        );
    }
}

/// Turn this frame's session events into light cues.
///
/// Reads the same messages the note visuals and the sounds read —
/// which is what makes this presentation and not gameplay. Every
/// player feeds the same room: the reference rig is one strip, and
/// it has no notion of who played the note.
pub fn drive_room_stage(
    stage: Res<RoomStage>,
    mut feedback: MessageReader<crate::gameplay::SessionFeedback>,
    players: Query<(
        &crate::gameplay::PlayerIndex,
        &crate::gameplay::PlayerSession,
    )>,
) {
    if !stage.is_live() {
        // Still drain, or the messages pile up for a reader that
        // never reads them.
        feedback.clear();
        return;
    }
    for message in feedback.read() {
        let Some((_, player)) = players
            .iter()
            .find(|(index, _)| index.0 == message.player_index)
        else {
            continue;
        };
        let track = player.session.track();
        // The note's shape and the tempo where it sits: the session
        // event carries an index, and the track turns it into both.
        let (lanes, bpm) = match message.event {
            SessionEvent::NoteHit { event_index, .. }
            | SessionEvent::NoteMissed { event_index }
            | SessionEvent::SustainStarted { event_index }
            | SessionEvent::SustainEnded { event_index, .. } => track
                .events()
                .get(event_index)
                .map_or((LaneSet::EMPTY, track.tempo.bpm_at(0.0)), |event| {
                    (event.lanes, track.tempo.bpm_at(event.time_s))
                }),
            _ => (LaneSet::EMPTY, track.tempo.bpm_at(0.0)),
        };
        if let Some(post) = post_for(message.event, lanes, bpm) {
            stage.send(post);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(index: usize) -> LaneSet {
        LaneSet::single(Lane::ALL[index])
    }

    #[test]
    fn a_better_hit_hits_the_room_harder() {
        assert!(kick_strength(Judgment::Perfect) > kick_strength(Judgment::Great));
        assert!(kick_strength(Judgment::Great) > kick_strength(Judgment::Good));
        assert!((kick_strength(Judgment::Perfect) - 1.0).abs() < f32::EPSILON);
        assert!(
            kick_strength(Judgment::Miss).abs() < f32::EPSILON,
            "a miss is not a kick"
        );
    }

    #[test]
    fn the_tone_runs_green_to_orange_and_a_chord_sits_between() {
        assert!((tone_of(lane(0)) - 0.0).abs() < 1e-6, "green is low");
        assert!((tone_of(lane(4)) - 1.0).abs() < 1e-6, "orange is high");
        assert!((tone_of(lane(2)) - 0.5).abs() < 1e-6);
        let chord = LaneSet::from_lanes([Lane::One, Lane::Five]);
        assert!((tone_of(chord) - 0.5).abs() < 1e-6, "a chord averages");
        assert!(
            (tone_of(LaneSet::EMPTY) - 0.5).abs() < 1e-6,
            "no lanes is not a colour, so it is the middle"
        );
    }

    #[test]
    fn every_event_maps_to_what_the_room_should_do() {
        let hit = post_for(
            SessionEvent::NoteHit {
                event_index: 0,
                judgment: Judgment::Perfect,
                offset_s: 0.0,
            },
            lane(0),
            128.0,
        )
        .expect("a hit lights the room");
        assert_eq!(hit.path, "/api/warn_kick");
        assert_eq!(
            hit.body,
            "{\"strength\":1.000,\"bpm\":128.00,\"vel\":1.000,\"tone\":0.000}"
        );

        // The room flinches on a miss and on an overstrum.
        for event in [
            SessionEvent::NoteMissed { event_index: 3 },
            SessionEvent::Overstrum,
        ] {
            let post = post_for(event, LaneSet::EMPTY, 120.0).expect("the room flinches");
            assert_eq!(post.path, "/api/warn_bass");
            assert_eq!(post.body, "{\"level\":0.000}");
        }

        // A sustain raises the beam and dropping it lowers it.
        let up = post_for(
            SessionEvent::SustainStarted { event_index: 1 },
            lane(1),
            120.0,
        )
        .expect("a beam");
        assert_eq!(
            (up.path, up.body.as_str()),
            ("/api/warn_bass", "{\"level\":1.000}")
        );
        let down = post_for(
            SessionEvent::SustainEnded {
                event_index: 1,
                completed: true,
            },
            lane(1),
            120.0,
        )
        .expect("the beam ends");
        assert_eq!(down.body, "{\"level\":0.000}");

        // Hype is the big accent; a finished phrase is a small one.
        assert_eq!(
            post_for(SessionEvent::HypeActivated, LaneSet::EMPTY, 120.0)
                .map(|p| p.body)
                .as_deref(),
            Some("{\"kind\":\"meteor\"}")
        );
        assert_eq!(
            post_for(
                SessionEvent::PhraseCompleted { phrase_index: 0 },
                LaneSet::EMPTY,
                120.0
            )
            .map(|p| p.path),
            Some("/api/warn_event")
        );

        // And the events the room has nothing to say about.
        assert_eq!(
            post_for(
                SessionEvent::PhraseBroken { phrase_index: 0 },
                LaneSet::EMPTY,
                120.0
            ),
            None
        );
        assert_eq!(post_for(SessionEvent::Failed, LaneSet::EMPTY, 120.0), None);
    }

    #[test]
    fn the_scene_engages_and_releases() {
        assert_eq!(scene_post(true).path, "/api/effect");
        assert!(scene_post(true).body.contains("iris_warn"));
        assert!(scene_post(false).body.contains("off"));
        assert_ne!(scene_post(true), scene_post(false));
    }

    #[test]
    fn an_idle_handle_swallows_everything_and_never_pretends() {
        let stage = RoomStage::idle();
        assert!(!stage.is_live());
        assert!(!stage.send(scene_post(true)), "nothing was sent");
        assert_eq!(stage.delivered(), 0);
        // An idle handle is not a DROP either — there was no worker
        // to be behind. Dropping counts congestion, not absence.
        assert_eq!(stage.dropped(), 0);
    }

    #[test]
    fn a_backlog_keeps_the_newest_cue_per_path_but_never_swallows_an_accent() {
        let kick = |strength: f32| Post::new("/api/warn_kick", format!("{strength:.3}"));
        let mut batch = Vec::new();
        push_coalesced(&mut batch, kick(0.6));
        push_coalesced(&mut batch, Post::new("/api/warn_bass", "a".to_owned()));
        push_coalesced(&mut batch, kick(1.0));
        push_coalesced(&mut batch, Post::new("/api/warn_bass", "b".to_owned()));
        assert_eq!(batch.len(), 2, "one kick and one bass, not four posts");
        assert_eq!(batch[0].body, "1.000", "the NEWEST kick survives");
        assert_eq!(batch[1].body, "b", "and the newest level");

        // Accents are gestures, not states: each one stands.
        let mut batch = Vec::new();
        push_coalesced(
            &mut batch,
            Post::new("/api/warn_event", "meteor".to_owned()),
        );
        push_coalesced(&mut batch, Post::new("/api/warn_event", "burst".to_owned()));
        assert_eq!(batch.len(), 2, "a meteor is not swallowed by a burst");
    }

    #[test]
    fn a_full_queue_drops_instead_of_blocking() {
        // The worker is pointed at an address nothing answers, so it
        // sits in its timeout while the queue fills. The frame thread
        // must never wait for it.
        let stage = RoomStage::connect("http://127.0.0.1:9");
        let start = std::time::Instant::now();
        for _ in 0..QUEUE * 4 {
            stage.send(scene_post(true));
        }
        let spent = start.elapsed();
        assert!(
            spent < std::time::Duration::from_millis(TIMEOUT_MS),
            "pushing {} events took {spent:?} — the frame thread waited",
            QUEUE * 4
        );
        assert!(
            stage.dropped() > 0,
            "a queue of {QUEUE} cannot have taken {} events",
            QUEUE * 4
        );
    }

    /// The wire, for real: a mock light service on a loopback port,
    /// and what actually arrives at it.
    ///
    /// This is the half the pure mapping cannot prove — that a post
    /// leaves the process at all, at the right path, with the right
    /// body.
    mod wire {
        use super::*;
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc::channel;

        /// A one-shot HTTP server that records what it is told.
        /// Returns its address and a receiver of (path, body).
        fn mock() -> (String, std::sync::mpsc::Receiver<(String, String)>) {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
            let addr = listener.local_addr().expect("its address");
            let (heard, inbox) = channel();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let mut request = String::new();
                    reader.read_line(&mut request).ok();
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned();
                    // Headers to the blank line, then the body by
                    // its length.
                    let mut length = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; length];
                    reader.read_exact(&mut body).ok();
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .ok();
                    if heard
                        .send((path, String::from_utf8_lossy(&body).into_owned()))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            (format!("http://{addr}"), inbox)
        }

        #[test]
        fn a_kick_reaches_the_light_service_at_the_contract_path() {
            let (base, heard) = mock();
            let stage = RoomStage::connect(&base);
            let post = post_for(
                SessionEvent::NoteHit {
                    event_index: 0,
                    judgment: Judgment::Great,
                    offset_s: 0.01,
                },
                LaneSet::single(Lane::Five),
                128.0,
            )
            .expect("a hit is a kick");
            assert!(stage.send(post), "the queue took it");

            let (path, body) = heard
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the service was told");
            assert_eq!(path, "/api/warn_kick");
            assert_eq!(
                body,
                "{\"strength\":0.800,\"bpm\":128.00,\"vel\":0.800,\"tone\":1.000}"
            );
            // The counter is bumped after the reply is read, which is
            // strictly later than the mock hearing the body — so wait
            // for it rather than racing it.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while stage.delivered() == 0 && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(stage.delivered(), 1, "the worker counted the delivery");
            assert_eq!(stage.dropped(), 0, "and dropped nothing");
        }

        #[test]
        fn the_scene_engages_over_the_wire_and_a_base_with_a_slash_is_not_doubled() {
            let (base, heard) = mock();
            let stage = RoomStage::connect(&format!("{base}/"));
            stage.send(scene_post(true));
            let (path, body) = heard
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the service was told");
            assert_eq!(path, "/api/effect", "one slash, not two");
            assert!(body.contains("iris_warn"));
        }
    }
}
