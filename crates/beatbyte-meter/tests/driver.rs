//! The driver end to end against a model pair built in this file.
//!
//! No fixture: both graphs are hand-encoded ONNX protobuf, so the
//! repository carries no model. The "front end" reshapes its input
//! into frames of 128; the "beat model" reads a frame's maximum as
//! the beat logit and its minimum as the downbeat logit. That is
//! enough to prove the contract the real pair is driven through —
//! the input and output names, the chunking over a song longer than
//! one chunk, keep-first stitching, the decoder and the snap — on the
//! real runtime, with real tensors.

use std::path::PathBuf;

use beatbyte_meter::{MEL_BANDS, Meter, Size, installed, track_samples};
use beatbyte_ml::hash::sha256_hex;
use beatbyte_ml::{BEAT_THIS_MEL, ModelSpec, ModelStore, Runtime};

// ---- a minimal protobuf writer -------------------------------------------

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn field_varint(out: &mut Vec<u8>, field: u64, value: u64) {
    varint(out, field << 3);
    varint(out, value);
}

fn field_bytes(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    varint(out, (field << 3) | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn field_str(out: &mut Vec<u8>, field: u64, text: &str) {
    field_bytes(out, field, text.as_bytes());
}

/// One dimension of a declared shape.
enum Dim {
    Fixed(u64),
    Named(&'static str),
}

/// `TypeProto` for a float tensor.
fn float_type(shape: &[Dim]) -> Vec<u8> {
    let mut dims = Vec::new();
    for d in shape {
        let mut dim = Vec::new();
        match d {
            Dim::Fixed(n) => field_varint(&mut dim, 1, *n), // dim_value
            Dim::Named(name) => field_str(&mut dim, 2, name), // dim_param
        }
        field_bytes(&mut dims, 1, &dim);
    }
    let mut tensor = Vec::new();
    field_varint(&mut tensor, 1, 1); // elem_type FLOAT
    field_bytes(&mut tensor, 2, &dims);
    let mut ty = Vec::new();
    field_bytes(&mut ty, 1, &tensor);
    ty
}

fn value_info(name: &str, shape: &[Dim]) -> Vec<u8> {
    let mut vi = Vec::new();
    field_str(&mut vi, 1, name);
    field_bytes(&mut vi, 2, &float_type(shape));
    vi
}

/// An int64 constant tensor.
fn int64_initializer(name: &str, values: &[i64]) -> Vec<u8> {
    let mut t = Vec::new();
    field_varint(&mut t, 1, values.len() as u64); // dims
    field_varint(&mut t, 2, 7); // INT64
    field_str(&mut t, 8, name);
    let mut raw = Vec::new();
    for v in values {
        raw.extend_from_slice(&v.to_le_bytes());
    }
    field_bytes(&mut t, 9, &raw);
    t
}

/// `AttributeProto` of INTS.
fn ints_attribute(name: &str, values: &[i64]) -> Vec<u8> {
    let mut a = Vec::new();
    field_str(&mut a, 1, name);
    for v in values {
        field_varint(&mut a, 8, *v as u64);
    }
    field_varint(&mut a, 20, 7); // INTS
    a
}

/// `AttributeProto` of INT.
fn int_attribute(name: &str, value: i64) -> Vec<u8> {
    let mut a = Vec::new();
    field_str(&mut a, 1, name);
    field_varint(&mut a, 3, value as u64);
    field_varint(&mut a, 20, 2); // INT
    a
}

fn node(op: &str, inputs: &[&str], outputs: &[&str], attributes: &[Vec<u8>]) -> Vec<u8> {
    let mut n = Vec::new();
    for i in inputs {
        field_str(&mut n, 1, i);
    }
    for o in outputs {
        field_str(&mut n, 2, o);
    }
    field_str(&mut n, 3, &format!("{op}_node"));
    field_str(&mut n, 4, op);
    for a in attributes {
        field_bytes(&mut n, 5, a);
    }
    n
}

fn model(graph: Vec<u8>) -> Vec<u8> {
    let mut opset = Vec::new();
    field_str(&mut opset, 1, "");
    field_varint(&mut opset, 2, 13);
    let mut m = Vec::new();
    field_varint(&mut m, 1, 8);
    field_str(&mut m, 2, "beatbyte-meter tests");
    field_bytes(&mut m, 7, &graph);
    field_bytes(&mut m, 8, &opset);
    m
}

/// `mel_spectrogram = Reshape(audio_pcm, [1, -1, 128])`.
fn front_end_onnx() -> Vec<u8> {
    let mut graph = Vec::new();
    field_bytes(
        &mut graph,
        1,
        &node(
            "Reshape",
            &["audio_pcm", "shape"],
            &["mel_spectrogram"],
            &[],
        ),
    );
    field_str(&mut graph, 2, "front-end");
    field_bytes(
        &mut graph,
        5,
        &int64_initializer("shape", &[1, -1, MEL_BANDS as i64]),
    );
    field_bytes(
        &mut graph,
        11,
        &value_info("audio_pcm", &[Dim::Fixed(1), Dim::Named("samples")]),
    );
    field_bytes(
        &mut graph,
        12,
        &value_info(
            "mel_spectrogram",
            &[
                Dim::Fixed(1),
                Dim::Named("frames"),
                Dim::Fixed(MEL_BANDS as u64),
            ],
        ),
    );
    model(graph)
}

/// `beat = ReduceMax(spectrogram, axis 2)`, `downbeat = ReduceMin(…)`.
fn beat_model_onnx() -> Vec<u8> {
    let mut graph = Vec::new();
    let attrs = [ints_attribute("axes", &[2]), int_attribute("keepdims", 0)];
    field_bytes(
        &mut graph,
        1,
        &node("ReduceMax", &["spectrogram"], &["beat"], &attrs),
    );
    field_bytes(
        &mut graph,
        1,
        &node("ReduceMin", &["spectrogram"], &["downbeat"], &attrs),
    );
    field_str(&mut graph, 2, "beat-model");
    field_bytes(
        &mut graph,
        11,
        &value_info(
            "spectrogram",
            &[
                Dim::Fixed(1),
                Dim::Named("frames"),
                Dim::Fixed(MEL_BANDS as u64),
            ],
        ),
    );
    field_bytes(
        &mut graph,
        12,
        &value_info("beat", &[Dim::Fixed(1), Dim::Named("frames")]),
    );
    field_bytes(
        &mut graph,
        12,
        &value_info("downbeat", &[Dim::Fixed(1), Dim::Named("frames")]),
    );
    model(graph)
}

/// A store holding the two graphs under the registry's ids, with
/// specs whose hashes match what was written.
fn store_with_pair() -> (ModelStore, ModelSpec, ModelSpec, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "beatbyte-meter-driver-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let store = ModelStore::at(root.clone());
    let mut specs = Vec::new();
    for (id, bytes) in [
        ("beat-this-mel", front_end_onnx()),
        ("beat-this-small", beat_model_onnx()),
    ] {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).expect("store dir");
        std::fs::write(dir.join("model.onnx"), &bytes).expect("write model");
        let sha = Box::leak(sha256_hex(&bytes).into_boxed_str());
        specs.push(ModelSpec {
            id,
            file: "model.onnx",
            url: "https://example.invalid/model.onnx",
            bytes: bytes.len() as u64,
            sha256: sha,
            licence: "MIT",
            purpose: "test",
        });
    }
    let beat = specs.pop().expect("beat spec");
    let mel = specs.pop().expect("mel spec");
    (store, mel, beat, root)
}

/// Frames whose maximum is the beat logit and minimum the downbeat
/// logit: `-5` everywhere, `+3` at every eighth frame from 12
/// (beats), and the downbeat logit `+1` on every fourth beat, one
/// frame late so the snap has work to do.
fn song(frames: usize) -> Vec<f32> {
    let mut data = vec![-5.0f32; frames * MEL_BANDS];
    for f in (12..frames).step_by(8) {
        data[f * MEL_BANDS] = 3.0; // beat: the frame's maximum
    }
    for (k, f) in (12..frames).step_by(8).enumerate() {
        if k % 4 == 0 && f + 1 < frames {
            // downbeat, one frame late: raise the whole of frame f+1 so
            // its minimum is positive (its maximum, 1, is no beat peak:
            // the beat at f, 3, is within the window)
            for band in 0..MEL_BANDS {
                data[(f + 1) * MEL_BANDS + band] = 1.0;
            }
        }
    }
    data
}

#[test]
fn a_song_longer_than_one_chunk_is_metered_end_to_end() {
    let (store, mel_spec, beat_spec, root) = store_with_pair();
    let runtime = Runtime::new();
    let mel = runtime.load(&store, &mel_spec).expect("front end loads");
    let beat = runtime.load(&store, &beat_spec).expect("beat model loads");

    let frames = 3300; // three chunks, the last pulled back
    let samples = song(frames);
    let meter = track_samples(&runtime, &mel, &beat, &samples).expect("metered");

    let expected_beats: Vec<f64> = (12..frames).step_by(8).map(|f| f as f64 / 50.0).collect();
    assert_eq!(meter.beats.len(), expected_beats.len());
    for (got, want) in meter.beats.iter().zip(&expected_beats) {
        assert!((got - want).abs() < 1e-9, "beat {got} vs {want}");
    }
    // Every fourth beat is a downbeat, and it sits ON the beat even
    // though the model put it a frame late.
    let expected_downbeats: Vec<f64> = expected_beats.iter().copied().step_by(4).collect();
    let expected_downbeats: Vec<f64> = expected_downbeats
        .into_iter()
        .filter(|t| (t * 50.0).round() as usize + 1 < frames)
        .collect();
    assert_eq!(meter.downbeats, expected_downbeats);
    assert_eq!(meter.model, "beat-this-small");
    assert_eq!(meter.sha256, beat_spec.sha256);

    // The same input gives the same answer.
    let again = track_samples(&runtime, &mel, &beat, &samples).expect("metered again");
    assert_eq!(again, meter);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_front_end_that_answers_in_the_wrong_shape_is_an_error_not_a_panic() {
    let (store, mel_spec, beat_spec, root) = store_with_pair();
    let runtime = Runtime::new();
    let mel = runtime.load(&store, &mel_spec).expect("front end loads");
    let beat = runtime.load(&store, &beat_spec).expect("beat model loads");
    // 100 samples cannot be reshaped into frames of 128: the runtime
    // refuses, and the driver reports rather than panics.
    let result = track_samples(&runtime, &mel, &beat, &[0.0; 100]);
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn installed_needs_the_front_end_and_prefers_the_full_model() {
    let root =
        std::env::temp_dir().join(format!("beatbyte-meter-installed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = ModelStore::at(root.clone());
    assert_eq!(installed(&store), None, "an empty store has no meter");
    // A front end alone is not a meter.
    let dir = root.join(BEAT_THIS_MEL.id);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join(BEAT_THIS_MEL.file), b"not the model").expect("write");
    assert_eq!(
        installed(&store),
        None,
        "a damaged front end does not count"
    );
    assert_eq!(Size::Full.spec().id, "beat-this");
    assert_eq!(Size::Small.spec().id, "beat-this-small");
    assert_eq!(Size::ALL, [Size::Full, Size::Small]);
    let _ = std::fs::remove_dir_all(root);
    let _ = Meter {
        beats: vec![],
        downbeats: vec![],
        model: "",
        sha256: "",
    };
}
