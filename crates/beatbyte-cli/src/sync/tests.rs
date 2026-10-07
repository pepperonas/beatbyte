//! Two devices and a hub, all in a scratch directory: every rule of
//! ADR-0021 carried out by the real command, rsync and all.

use super::*;
use beatbyte_telemetry::{
    Completion, Detail, Event, EventType, InputDevice, Outcome, Provenance, SessionRow,
};
use std::collections::BTreeMap;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bb-sync-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// A device: a data directory with a fixed device id.
fn device(root: &Path, name: &str) -> PathBuf {
    let data = root.join(name);
    write_json(
        &data.join("sync").join("device.json"),
        &json!({ "device": name }),
    )
    .expect("config");
    data
}

fn sync_now(data: &Path, hub: &Path) -> String {
    sync(&Options {
        data: data.to_owned(),
        settings: data.join("settings.json"),
        hub: Some(hub.to_string_lossy().into_owned()),
        dry_run: false,
        models: false,
        check_game: false,
    })
    .expect("sync")
}

fn put(data: &Path, rel: &str, text: &str) {
    let path = data.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, text).expect("write");
}

fn song(data: &Path, folder: &str, rel: &str, text: &str) {
    put(data, &format!("songs/imported/{folder}/{rel}"), text);
}

fn read(data: &Path, rel: &str) -> String {
    std::fs::read_to_string(data.join(rel)).unwrap_or_default()
}

fn roster(data: &Path, name: &str, created_ms: u64) -> u64 {
    let mut roster: Roster = read_json(&data.join("players.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let id = roster.add(name, created_ms).expect("adds");
    write_json(
        &data.join("players.json"),
        &serde_json::to_value(&roster).expect("roster"),
    )
    .expect("writes");
    id
}

fn history_line(started_ms: u64, title: &str, player: u64) -> String {
    format!(
        "{}\n",
        json!({"started_ms": started_ms, "title": title, "artist": "A",
            "difficulty": "hard", "score": 100, "player": player})
    )
}

fn telemetry(data: &Path, uid: &str, events: u32) {
    let mut store = beatbyte_telemetry::Store::open(&data.join("telemetry.db")).expect("store");
    let row = SessionRow {
        uid: uid.to_owned(),
        started_ms: 1_700_000_000_000,
        title: "Maria".to_owned(),
        artist: "Blondie".to_owned(),
        genre: None,
        chart_hash: "abc".to_owned(),
        song_id: None,
        chart_file: None,
        difficulty: 1,
        player_slot: 0,
        player_id: None,
        provenance: Provenance {
            game: "0.18.0".to_owned(),
            chart_format: 1,
            generator: None,
            scoring: 1,
            analysis: None,
            vocal: None,
        },
        telemetry_schema: beatbyte_telemetry::schema_version(),
        detail: Detail::Actions,
        input_device: InputDevice::Keyboard,
        input_offset_ms: None,
        video_offset_ms: None,
        mic_offset_ms: None,
        tap_mode: false,
        no_fail: false,
        practice: false,
        autopilot: false,
        notes_total: 10,
    };
    let id = store.begin(&row).expect("begins");
    let batch: Vec<(u32, Event)> = (0..events)
        .map(|i| (i, Event::new(EventType::NoteHit, i64::from(i) * 1_000)))
        .collect();
    store.append(id, &batch).expect("appends");
    store
        .finish(
            id,
            Outcome {
                ended_ms: 2,
                completion: Completion::Completed,
                dropped: 0,
                practice: false,
            },
        )
        .expect("finishes");
}

/// What two devices must hold identically after a sync. The library
/// is NOT part of it: song files do not travel (2026-10-08).
type Shared = (String, String, String, String, String, Value);

fn shared_state(data: &Path) -> Shared {
    let settings = read_json(&data.join("settings.json")).unwrap_or(Value::Null);
    (
        read(data, "players.json"),
        read(data, "history.jsonl"),
        read(data, "scores.json"),
        read(data, "achievements.json"),
        read(data, "ratings.json"),
        beatbyte_sync::settings::shared_part(&settings),
    )
}

fn setting(data: &Path, value: &Value) {
    write_json(&data.join("settings.json"), value).expect("settings");
}

/// The whole career travels, every rule applies, and the two devices
/// end identical — with nothing counted twice.
#[test]
#[allow(clippy::too_many_lines)] // one story, told in order
fn two_devices_end_with_one_career() {
    let root = scratch("career");
    let hub = root.join("hub");
    let (a, b) = (device(&root, "mac-a"), device(&root, "mac-b"));

    // The same person, created on each device at a different moment.
    let martin_a = roster(&a, "Martin", 1_000);
    let martin_b = roster(&b, "martin", 2_000);
    assert_ne!(martin_a, martin_b);

    put(
        &a,
        "history.jsonl",
        &(history_line(10, "Heroes", martin_a) + &history_line(30, "Mexico", martin_a)),
    );
    put(
        &b,
        "history.jsonl",
        &(history_line(10, "Heroes", martin_b) + &history_line(20, "Africa", martin_b)),
    );

    let record = |song: &str, score: u64| {
        json!({"song_id": song, "title": song, "artist": "A", "difficulty": "hard",
            "score": score, "accuracy": 0.9, "best_streak": 10})
    };
    write_json(
        &a.join("scores.json"),
        &json!({"version": 3, "records": [record("s1", 900), record("s2", 50)]}),
    )
    .expect("w");
    write_json(
        &b.join("scores.json"),
        &json!({"version": 3, "records": [record("s1", 950)]}),
    )
    .expect("w");

    write_json(
        &a.join("achievements.json"),
        &json!({ martin_a.to_string(): {"at": {"combo_50": 500}}}),
    )
    .expect("w");
    write_json(
        &b.join("achievements.json"),
        &json!({ martin_b.to_string(): {"at": {"combo_50": 300, "diff_hard": 900}}}),
    )
    .expect("w");

    setting(
        &a,
        &json!({"theme": "neon", "latency_offset_ms": 40, "anthropic_api_key": "sk-a-secret",
        "changed_ms": {"theme": 100}}),
    );
    setting(
        &b,
        &json!({"theme": "ember", "latency_offset_ms": 120, "anthropic_api_key": "sk-b-secret",
        "changed_ms": {"theme": 500}}),
    );

    // Favourites and stars: A favourites Heroes, B takes the favourite
    // back LATER and stars a chart A never rated.
    let mut ra = beatbyte_sync::ratings::Ratings::default();
    ra.set(&martin_a.to_string(), "song:a|heroes", "favorite", 1, 100);
    ra.set(&martin_a.to_string(), "song:a|heroes", "song", 4, 100);
    write_json(
        &a.join("ratings.json"),
        &serde_json::to_value(&ra).expect("v"),
    )
    .expect("w");
    let mut rb = beatbyte_sync::ratings::Ratings::default();
    rb.set(&martin_b.to_string(), "song:a|heroes", "favorite", 0, 200);
    rb.set(&martin_b.to_string(), "chart:id:s1", "chart", 3, 150);
    write_json(
        &b.join("ratings.json"),
        &serde_json::to_value(&rb).expect("v"),
    )
    .expect("w");

    telemetry(&a, "u-a", 5);
    telemetry(&b, "u-b", 7);
    telemetry(&b, "u-both", 3);
    telemetry(&a, "u-both", 3);

    // The library: a song only A has, one only B has, and a chart
    // version generated on each device differently.
    song(
        &a,
        "only-a",
        "chart.json",
        r#"{"provenance":{"created_ms":1}}"#,
    );
    song(&a, "only-a", "song.m4a", "audio-a");
    // A pointer in the spelling the game's older writer used: it must
    // arrive as these bytes, not as an equivalent rewrite.
    song(
        &a,
        "only-a",
        "chart-active.json",
        "{\"active\": \"chart.json\"}",
    );
    song(
        &b,
        "only-b",
        "chart.json",
        r#"{"provenance":{"created_ms":1}}"#,
    );
    song(&a, "both", "chart.json", "base");
    song(&b, "both", "chart.json", "base");
    song(
        &a,
        "both",
        "chart.v2.json",
        r#"{"provenance":{"created_ms":10},"from":"a"}"#,
    );
    song(
        &b,
        "both",
        "chart.v2.json",
        r#"{"provenance":{"created_ms":20},"from":"b"}"#,
    );
    song(&a, "both", "chart.v2.context.json", "ctx-a");
    song(&b, "both", "chart.v2.context.json", "ctx-b");

    // A publishes, B merges and publishes, A merges: two syncs each
    // way, in either order, must converge.
    sync_now(&a, &hub);
    sync_now(&b, &hub);
    sync_now(&a, &hub);
    sync_now(&b, &hub);

    assert_eq!(shared_state(&a), shared_state(&b), "the devices differ");

    // One person, not two.
    let players: Roster = serde_json::from_str(&read(&a, "players.json")).expect("roster");
    assert_eq!(players.players.len(), 1, "{players:?}");
    let martin = players.players[0].id;

    // Every run once — Heroes was played at the same millisecond on
    // "both" devices here, so it is ONE run — all on the one person.
    let history = read(&a, "history.jsonl");
    assert_eq!(beatbyte_sync::history::count(&history), 3, "{history}");
    for line in history.lines() {
        let line: Value = serde_json::from_str(line).expect("line");
        assert_eq!(line["player"], martin, "{line}");
    }

    // The best score per song survives, from either side.
    let scores = read_json(&a.join("scores.json")).expect("scores");
    let by_song: BTreeMap<String, u64> = scores["records"]
        .as_array()
        .expect("records")
        .iter()
        .map(|r| {
            (
                r["song_id"].as_str().expect("id").to_owned(),
                r["score"].as_u64().expect("score"),
            )
        })
        .collect();
    assert_eq!(
        by_song,
        BTreeMap::from([("s1".to_owned(), 950), ("s2".to_owned(), 50)])
    );

    // Favourites and stars on the one person: the later un-favourite,
    // the stars only A gave, the chart stars only B gave.
    let ratings: beatbyte_sync::ratings::Ratings =
        serde_json::from_value(read_json(&a.join("ratings.json")).expect("ratings"))
            .expect("parse");
    let m = martin.to_string();
    assert_eq!(ratings.players.len(), 1, "{ratings:?}");
    assert_eq!(ratings.get(&m, "song:a|heroes", "favorite"), 0);
    assert_eq!(ratings.get(&m, "song:a|heroes", "song"), 4);
    assert_eq!(ratings.get(&m, "chart:id:s1", "chart"), 3);

    // The earliest date an achievement was earned.
    let achievements = read_json(&a.join("achievements.json")).expect("achievements");
    assert_eq!(achievements[martin.to_string()]["at"]["combo_50"], 300);
    assert_eq!(achievements[martin.to_string()]["at"]["diff_hard"], 900);

    // The newer shared setting; the device's own untouched.
    for (data, latency, key) in [(&a, 40, "sk-a-secret"), (&b, 120, "sk-b-secret")] {
        let settings = read_json(&data.join("settings.json")).expect("settings");
        assert_eq!(settings["theme"], "ember");
        assert_eq!(
            settings["latency_offset_ms"], latency,
            "calibration travelled"
        );
        assert_eq!(settings["anthropic_api_key"], key, "the API key travelled");
    }

    // ⚠️ The user's rule: the API key never reaches the hub.
    let mut on_hub = Vec::new();
    walk(&hub, "", &mut on_hub);
    for rel in &on_hub {
        let bytes = std::fs::read(hub.join(rel)).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("sk-a-secret") && !text.contains("sk-b-secret"),
            "the API key is on the hub in {rel}"
        );
    }

    // Telemetry: rows merged, the shared session once.
    let (sessions, events) = telemetry_counts(&a.join("telemetry.db"));
    assert_eq!((sessions, events), (3, 15));
    assert_eq!(telemetry_counts(&b.join("telemetry.db")), (3, 15));

    // ⚠️ The user's rule (2026-10-08): the music library is never
    // copied — not to the hub, not to the other device. Each device
    // keeps its own songs and publishes only their list.
    assert!(
        !library_root(&b).join("only-a").exists(),
        "a song travelled to B"
    );
    assert!(
        !library_root(&a).join("only-b").exists(),
        "a song travelled to A"
    );
    assert!(!read(&b, "songs/imported/both/chart.v2.json").contains("\"a\""));
    for rel in &on_hub {
        assert!(
            !rel.starts_with("blobs/"),
            "a song file is on the hub: {rel}"
        );
        assert!(
            !rel.ends_with("library.json"),
            "a library manifest is on the hub: {rel}"
        );
        assert!(
            !rel.ends_with(".m4a") && !rel.ends_with("chart.json"),
            "a song file is on the hub: {rel}"
        );
    }
    // The track lists are, one per device.
    let list_a =
        std::fs::read_to_string(hub.join("devices/mac-a").join(TRACKLIST)).expect("A's list");
    let list_b =
        std::fs::read_to_string(hub.join("devices/mac-b").join(TRACKLIST)).expect("B's list");
    assert!(list_a.lines().any(|l| l.starts_with("only-a,")), "{list_a}");
    assert!(
        !list_a.lines().any(|l| l.starts_with("only-b,")),
        "{list_a}"
    );
    assert!(list_b.lines().any(|l| l.starts_with("only-b,")), "{list_b}");

    // A third sync on either side changes nothing.
    let before = shared_state(&a);
    let (sa, ea) = telemetry_counts(&a.join("telemetry.db"));
    sync_now(&a, &hub);
    sync_now(&b, &hub);
    assert_eq!(shared_state(&a), before);
    assert_eq!(telemetry_counts(&a.join("telemetry.db")), (sa, ea));
}

/// A dry run writes nothing — not here, not on the hub.
#[test]
fn a_dry_run_writes_nothing() {
    let root = scratch("dry");
    let hub = root.join("hub");
    let (a, b) = (device(&root, "mac-a"), device(&root, "mac-b"));
    song(&a, "s", "chart.json", "c");
    sync_now(&a, &hub);
    let before_hub = {
        let mut files = Vec::new();
        walk(&hub, "", &mut files);
        files
    };
    let report = sync(&Options {
        settings: b.join("settings.json"),
        data: b.clone(),
        hub: Some(hub.to_string_lossy().into_owned()),
        dry_run: true,
        models: false,
        check_game: false,
    })
    .expect("dry run");
    assert!(report.contains("dry run"), "{report}");
    assert!(!library_root(&b).exists(), "the dry run fetched");
    let mut after_hub = Vec::new();
    walk(&hub, "", &mut after_hub);
    assert_eq!(before_hub, after_hub, "the dry run touched the hub");
}

/// Somebody else's sync holds the hub: wait, do not barge in.
#[test]
fn a_held_lock_is_respected_and_a_stale_one_broken() {
    let root = scratch("lock");
    let hub = root.join("hub");
    let a = device(&root, "mac-a");
    std::fs::create_dir_all(hub.join("lock")).expect("lock");
    std::fs::write(hub.join("lock/owner"), format!("mac-b {}", now_ms())).expect("owner");
    let err = sync(&Options {
        settings: a.join("settings.json"),
        data: a.clone(),
        hub: Some(hub.to_string_lossy().into_owned()),
        dry_run: false,
        models: false,
        check_game: false,
    })
    .expect_err("held");
    assert!(err.contains("another sync"), "{err}");
    assert!(
        hub.join("lock/owner").exists(),
        "somebody else's lock was removed"
    );

    std::fs::write(hub.join("lock/owner"), "mac-b 1").expect("stale");
    sync_now(&a, &hub);
    assert!(!hub.join("lock").exists());
}

#[test]
fn a_hub_is_ssh_only_when_it_names_a_host() {
    assert_eq!(
        Hub::parse("raspi5:beatbyte-hub"),
        Hub::Ssh {
            host: "raspi5".to_owned(),
            path: "beatbyte-hub".to_owned()
        }
    );
    assert_eq!(
        Hub::parse("/tmp/hub"),
        Hub::Local(PathBuf::from("/tmp/hub"))
    );
    assert_eq!(Hub::parse("./a:b"), Hub::Local(PathBuf::from("./a:b")));
}

/// The game keeps its settings in the CONFIG directory — on Linux not
/// the data directory. The sync reads and writes the file it is given.
#[test]
fn settings_are_read_where_the_game_keeps_them() {
    let root = scratch("config");
    let hub = root.join("hub");
    let (a, b) = (device(&root, "mac-a"), device(&root, "mac-b"));
    let elsewhere = root.join("config-a").join("settings.json");
    write_json(
        &elsewhere,
        &json!({"theme": "ember", "changed_ms": {"theme": 9}}),
    )
    .expect("w");
    sync(&Options {
        data: a.clone(),
        settings: elsewhere.clone(),
        hub: Some(hub.to_string_lossy().into_owned()),
        dry_run: false,
        models: false,
        check_game: false,
    })
    .expect("sync a");
    sync_now(&b, &hub);
    assert_eq!(
        read_json(&b.join("settings.json")).expect("b")["theme"],
        "ember"
    );
    assert!(
        !a.join("settings.json").exists(),
        "a settings file appeared in the data dir"
    );
}

/// A device folder with a name this tool never makes is refused.
#[test]
fn a_device_name_this_tool_never_makes_is_refused() {
    let root = scratch("devname");
    let hub = root.join("hub");
    let a = device(&root, "mac-a");
    std::fs::create_dir_all(hub.join("devices").join("Evil Name")).expect("odd");
    let err = sync(&Options {
        settings: a.join("settings.json"),
        data: a.clone(),
        hub: Some(hub.to_string_lossy().into_owned()),
        dry_run: false,
        models: false,
        check_game: false,
    })
    .expect_err("must refuse");
    assert!(err.contains("refusing"), "{err}");
    assert!(is_device_id("macbookpro-1a0d7b86052"));
    assert!(!is_device_id("../x") && !is_device_id("A") && !is_device_id(""));
}

/// An older build still publishes a library manifest and blobs. This
/// build ignores them, fetches nothing, and removes its OWN old
/// manifest from the hub on its next publish.
#[test]
fn a_library_an_older_build_published_is_ignored_and_its_own_is_removed() {
    let root = scratch("old-library");
    let hub = root.join("hub");
    let (a, b) = (device(&root, "mac-a"), device(&root, "mac-b"));
    song(
        &a,
        "mine",
        "chart.json",
        r#"{"song":{"title":"Mine","artist":"A"}}"#,
    );
    // What an older B left on the hub: a manifest naming a song and its blob.
    put(&hub, "blobs/0000", "not a song any more");
    put(
        &hub,
        "devices/mac-b/library.json",
        r#"{"files":{"theirs/chart.json":{"hash":"0000","size":3,"mtime":1}}}"#,
    );
    put(&hub, "devices/mac-a/library.json", "{}");
    roster(&b, "Martin", 1_000);
    sync_now(&b, &hub);
    // B's new build published its list and removed its manifest.
    assert!(!hub.join("devices/mac-b/library.json").exists());
    sync_now(&a, &hub);
    assert!(
        !library_root(&a).join("theirs").exists(),
        "a song was fetched"
    );
    assert!(
        !hub.join("devices/mac-a/library.json").exists(),
        "the old manifest stayed"
    );
    assert!(hub.join("devices/mac-a").join(TRACKLIST).exists());
}

/// The track list names every song folder with its version and source,
/// quoted where a field needs it, sorted by folder.
#[test]
fn the_track_list_says_what_every_folder_is() {
    let root = scratch("tracklist");
    let data = device(&root, "mac-a");
    song(
        &data,
        "maria",
        "chart.json",
        r#"{"song":{"title":"Maria","artist":"Blondie"}}"#,
    );
    song(
        &data,
        "guitar-study-maria",
        "chart.json",
        r#"{"song":{"title":"[GS] Maria","artist":"Blondie"}}"#,
    );
    song(
        &data,
        "gamerip-01-crazy",
        "chart.json",
        r#"{"song":{"title":"[GR-01] Crazy Train","artist":"Ozzy Osbourne"}}"#,
    );
    song(
        &data,
        "gamerip-01-crazy",
        "bridge-source.json",
        r#"{"source":"/x/batch/Ozzy Osbourne - Crazy Train (Harmonix)","fingerprint":"f","format":"mid","charter":"Harmonix","game":"Rock Band 3","imported_ms":1}"#,
    );
    song(
        &data,
        "bridge-02-x",
        "chart.json",
        r#"{"song":{"title":"[BG-02] Hello, \"World\"","artist":"A"}}"#,
    );
    song(&data, "no-chart", "song.m4a", "audio only");
    let list = tracklist(&library_root(&data));
    let lines: Vec<&str> = list.lines().collect();
    assert_eq!(
        lines[0],
        "folder,version,artist,title,charter,game,bridge_download,song_id"
    );
    assert_eq!(
        lines.len(),
        5,
        "four songs; a folder without a chart is not one:\n{list}"
    );
    assert_eq!(lines[1], r#"bridge-02-x,BG-02,A,"Hello, ""World""",,,,"#);
    assert_eq!(
        lines[2],
        "gamerip-01-crazy,GR-01,Ozzy Osbourne,Crazy Train,Harmonix,Rock Band 3,Ozzy Osbourne - Crazy Train (Harmonix),"
    );
    assert_eq!(lines[3], "guitar-study-maria,GS,Blondie,Maria,,,,");
    assert_eq!(lines[4], "maria,NORMAL,Blondie,Maria,,,,");
}
