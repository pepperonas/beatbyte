//! Settings (`settings.json`): SHARED keys follow the newest change,
//! DEVICE keys never leave their machine.
//!
//! The split is explicit and closed: a key is shared only if it is
//! named in [`SHARED`]. Anything else — including a key a later build
//! adds and nobody classified yet — stays on its device, because a
//! setting copied to the wrong machine (a calibration, a volume, a
//! display) is worse than one that has to be set twice. A test in the
//! game pins that every key the game writes is named in one of the
//! two lists.
//!
//! ⚠️ `anthropic_api_key` is a device key and never travels over the
//! sync path (the user's rule). A test pins it.
//!
//! **Newest change, per key.** The game stamps each shared key with
//! the millisecond it last CHANGED (`changed_ms`, written when it
//! saves, by comparing with what it loaded — the game rewrites the
//! whole file on exit, so the stamp, not the file's age, is the
//! evidence). For a preference that is the honest rule; it is per
//! key, so a volume changed here and a theme changed there both
//! survive. On an exact tie the value whose JSON sorts first wins, so
//! both devices agree.

use serde_json::{Map, Value};

/// The field of `settings.json` that carries the change stamps.
pub const STAMPS: &str = "changed_ms";

/// Keys that describe the player and follow them from device to
/// device.
pub const SHARED: &[&str] = &[
    "scroll_speed",
    "screen_shake",
    "beat_pulse",
    "backdrop_motion",
    "hit_labels",
    "no_fail",
    "reduced_flashing",
    "ai_search",
    "flash_sync",
    "high_contrast",
    "lyrics",
    "lyrics_size",
    "lyrics_lead_in_ms",
    "song_preview",
    "normalize_loudness",
    "guitar_study_twins",
    "vocal_charts",
    "vocal_pitch_mode",
    "original_vocals",
    "tap_mode",
    "perspective",
    "theme",
    "browser_sort",
    "browser_sort_reversed",
    "telemetry",
];

/// Keys that describe the MACHINE — its audio path, its screen, its
/// GPU, its input devices, its files — and never leave it.
pub const DEVICE: &[&str] = &[
    "music_volume",
    "sfx_volume",
    "mute_browsing",
    "mute_playing",
    "mute_autopilot",
    "latency_offset_ms",
    "video_offset_ms",
    "mic_offset_ms",
    "lyrics_offset_ms",
    "fullscreen",
    "ui_scale",
    "stage_3d",
    "particles",
    "fx_intensity",
    "input_map",
    "watch_folder",
    "room_lights",
    "room_stage_url",
    "anthropic_api_key",
    STAMPS,
];

fn stamp(settings: &Map<String, Value>, key: &str) -> u64 {
    settings
        .get(STAMPS)
        .and_then(|s| s.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// What merging two settings files did.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    /// The LOCAL file with the newer shared values taken over.
    pub settings: Value,
    /// Shared keys the remote side changed more recently.
    pub taken: Vec<String>,
}

/// Merge the remote file's shared keys into the local one. Device keys
/// and unknown keys are the local file's, untouched. Pure — tested.
#[must_use]
pub fn merge(local: &Value, remote: &Value) -> Merged {
    let mut out = local.as_object().cloned().unwrap_or_default();
    let Some(theirs) = remote.as_object() else {
        return Merged {
            settings: Value::Object(out),
            taken: Vec::new(),
        };
    };
    let mut taken = Vec::new();
    for key in SHARED {
        let Some(their_value) = theirs.get(*key) else {
            continue;
        };
        let (mine_ms, their_ms) = (stamp(&out, key), stamp(theirs, key));
        let differs = out.get(*key) != Some(their_value);
        let theirs_newer = their_ms > mine_ms
            || (their_ms == mine_ms
                && differs
                && serde_json::to_string(their_value).unwrap_or_default()
                    < out
                        .get(*key)
                        .map(|v| serde_json::to_string(v).unwrap_or_default())
                        .unwrap_or_default());
        if theirs_newer && differs {
            out.insert((*key).to_owned(), their_value.clone());
            taken.push((*key).to_owned());
        }
        if their_ms > mine_ms {
            let stamps = out
                .entry(STAMPS.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(stamps) = stamps.as_object_mut() {
                stamps.insert((*key).to_owned(), Value::from(their_ms));
            }
        }
    }
    Merged {
        settings: Value::Object(out),
        taken,
    }
}

/// The shared part of a settings file — what a device publishes. The
/// device keys, and above all the API key, are not in it at all.
#[must_use]
pub fn shared_part(settings: &Value) -> Value {
    let mut out = Map::new();
    if let Some(all) = settings.as_object() {
        for key in SHARED {
            if let Some(value) = all.get(*key) {
                out.insert((*key).to_owned(), value.clone());
            }
        }
        let stamps: Map<String, Value> = SHARED
            .iter()
            .filter_map(|key| {
                all.get(STAMPS)
                    .and_then(|s| s.get(*key))
                    .map(|v| ((*key).to_owned(), v.clone()))
            })
            .collect();
        out.insert(STAMPS.to_owned(), Value::Object(stamps));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// ⚠️ The user's rule: the API key never travels.
    #[test]
    fn the_api_key_is_a_device_key_and_never_published() {
        assert!(!SHARED.contains(&"anthropic_api_key"));
        assert!(DEVICE.contains(&"anthropic_api_key"));
        let published = shared_part(&json!({"anthropic_api_key": "sk-secret", "theme": "neon"}));
        assert!(!published.to_string().contains("sk-secret"), "{published}");
        // And a remote file that carries one anyway is not taken.
        let merged = merge(
            &json!({"anthropic_api_key": "mine"}),
            &json!({"anthropic_api_key": "theirs", "changed_ms": {"anthropic_api_key": 9}}),
        );
        assert_eq!(merged.settings["anthropic_api_key"], "mine");
    }

    #[test]
    fn the_two_lists_do_not_overlap() {
        for key in SHARED {
            assert!(!DEVICE.contains(key), "{key} is in both lists");
        }
    }

    /// Calibration and the machine's settings stay where they are,
    /// however new the other device's are.
    #[test]
    fn device_keys_never_move() {
        let local = json!({"latency_offset_ms": 40, "fullscreen": true});
        let remote = json!({"latency_offset_ms": 120, "fullscreen": false,
            "changed_ms": {"latency_offset_ms": 99, "fullscreen": 99}});
        assert_eq!(merge(&local, &remote).settings, local);
    }

    /// Per key, the newer change — so a change on each device to
    /// different keys both survive.
    #[test]
    fn the_newer_change_wins_per_key() {
        let local = json!({"theme": "neon", "scroll_speed": 1.0,
            "changed_ms": {"theme": 100, "scroll_speed": 900}});
        let remote = json!({"theme": "ember", "scroll_speed": 2.0,
            "changed_ms": {"theme": 500, "scroll_speed": 200}});
        let m = merge(&local, &remote);
        assert_eq!(m.settings["theme"], "ember");
        assert_eq!(m.settings["scroll_speed"], 1.0);
        assert_eq!(m.settings["changed_ms"]["theme"], 500);
        assert_eq!(m.taken, vec!["theme".to_owned()]);
    }

    /// A key another build added and nobody classified stays local.
    #[test]
    fn an_unknown_key_is_never_taken() {
        let local = json!({"future": 1});
        let remote = json!({"future": 2, "changed_ms": {"future": 9}});
        assert_eq!(merge(&local, &remote).settings["future"], 1);
    }

    #[test]
    fn both_devices_converge_on_the_shared_keys() {
        let a =
            json!({"theme": "neon", "tap_mode": true, "changed_ms": {"theme": 5, "tap_mode": 1}});
        let b =
            json!({"theme": "ember", "tap_mode": false, "changed_ms": {"theme": 5, "tap_mode": 2}});
        let ab = merge(&a, &b).settings;
        let ba = merge(&b, &a).settings;
        assert_eq!(shared_part(&ab), shared_part(&ba));
        assert_eq!(merge(&ab, &ba).settings, ab);
    }
}
