//! The driver end to end against a model built in this file.
//!
//! No fixture: the graph is hand-encoded ONNX protobuf, so the
//! repository carries no model. It has the real Basic Pitch input and
//! output NAMES and shapes, and answers with two slices of its own
//! input: the first 172 × 88 samples of a window are the "note"
//! answer, the next 172 × 88 the "onset" answer. So a song can be
//! written that spells out, sample by sample, what the model will
//! say — and the whole path is proved on the real runtime: the names,
//! the windowing over a song longer than one window, the stitching,
//! the drift correction and the note tracker.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use beatbyte_ml::hash::sha256_hex;
use beatbyte_ml::{ModelSpec, ModelStore, Runtime};
use beatbyte_poly::frames::{
    HOP_SAMPLES, OVERLAP_FRAMES, OVERLAP_SAMPLES, PITCHES, WINDOW_FRAMES, WINDOW_SAMPLES,
    frame_time,
};
use beatbyte_poly::{INPUT, transcribe_samples};

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
}

/// `TypeProto` for a float tensor.
fn float_type(shape: &[Dim]) -> Vec<u8> {
    let mut dims = Vec::new();
    for d in shape {
        let mut dim = Vec::new();
        match d {
            Dim::Fixed(n) => field_varint(&mut dim, 1, *n), // dim_value
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

fn node(
    name: &str,
    op: &str,
    inputs: &[&str],
    outputs: &[&str],
    attributes: &[Vec<u8>],
) -> Vec<u8> {
    let mut n = Vec::new();
    for i in inputs {
        field_str(&mut n, 1, i);
    }
    for o in outputs {
        field_str(&mut n, 2, o);
    }
    field_str(&mut n, 3, name);
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
    field_str(&mut m, 2, "beatbyte-poly tests");
    field_bytes(&mut m, 7, &graph);
    field_bytes(&mut m, 8, &opset);
    m
}

const REGION: usize = WINDOW_FRAMES * PITCHES;

/// `note = Reshape(Slice(x, 0..REGION))`, `onset = Reshape(Slice(x,
/// REGION..2·REGION))`, both `[1, 172, 88]`.
fn fake_basic_pitch() -> Vec<u8> {
    let mut graph = Vec::new();
    // ⚠️ The REAL graph's names, written out rather than taken from
    // the driver's constants: which output is which was established
    // against the real model (a sustained tone came out as one note
    // of its length read this way round), and a test that borrowed the
    // driver's own constants would follow any swap without noticing.
    for (name, from, to, output) in [
        ("note", 0, REGION, "StatefulPartitionedCall:1"),
        ("onset", REGION, 2 * REGION, "StatefulPartitionedCall:2"),
    ] {
        let starts = format!("{name}_starts");
        let ends = format!("{name}_ends");
        let sliced = format!("{name}_sliced");
        field_bytes(
            &mut graph,
            1,
            &node(
                &format!("{name}_slice"),
                "Slice",
                &[INPUT, &starts, &ends, "axes"],
                &[&sliced],
                &[],
            ),
        );
        field_bytes(
            &mut graph,
            1,
            &node(
                &format!("{name}_reshape"),
                "Reshape",
                &[&sliced, "shape"],
                &[output],
                &[],
            ),
        );
        field_bytes(&mut graph, 5, &int64_initializer(&starts, &[from as i64]));
        field_bytes(&mut graph, 5, &int64_initializer(&ends, &[to as i64]));
    }
    field_str(&mut graph, 2, "fake-basic-pitch");
    field_bytes(&mut graph, 5, &int64_initializer("axes", &[1]));
    field_bytes(
        &mut graph,
        5,
        &int64_initializer("shape", &[1, WINDOW_FRAMES as i64, PITCHES as i64]),
    );
    field_bytes(
        &mut graph,
        11,
        &value_info(
            INPUT,
            &[
                Dim::Fixed(1),
                Dim::Fixed(WINDOW_SAMPLES as u64),
                Dim::Fixed(1),
            ],
        ),
    );
    for output in ["StatefulPartitionedCall:1", "StatefulPartitionedCall:2"] {
        field_bytes(
            &mut graph,
            12,
            &value_info(
                output,
                &[
                    Dim::Fixed(1),
                    Dim::Fixed(WINDOW_FRAMES as u64),
                    Dim::Fixed(PITCHES as u64),
                ],
            ),
        );
    }
    model(graph)
}

/// A store holding the graph under the registry's id.
fn store_with_model() -> (ModelStore, ModelSpec, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "beatbyte-poly-driver-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let bytes = fake_basic_pitch();
    let dir = root.join("basic-pitch");
    std::fs::create_dir_all(&dir).expect("store dir");
    std::fs::write(dir.join("model.onnx"), &bytes).expect("write model");
    let spec = ModelSpec {
        id: "basic-pitch",
        file: "model.onnx",
        url: "https://example.invalid/model.onnx",
        bytes: bytes.len() as u64,
        sha256: Box::leak(sha256_hex(&bytes).into_boxed_str()),
        licence: "Apache-2.0",
        purpose: "test",
    };
    (ModelStore::at(root.clone()), spec, root)
}

/// Make the fake model answer `value` at stitched frame `frame`, key
/// `pitch`, in the note (`region` 0) or onset (`region` 1) answer:
/// find the window whose kept middle holds that frame and write the
/// sample the graph will read there.
fn say(samples: &mut [f32], region: usize, frame: usize, pitch: usize, value: f32) {
    let kept = WINDOW_FRAMES - OVERLAP_FRAMES;
    let window = frame / kept;
    let local = frame % kept + OVERLAP_FRAMES / 2;
    let in_window = region * REGION + local * PITCHES + pitch;
    let index = window * HOP_SAMPLES + in_window;
    let index = index
        .checked_sub(OVERLAP_SAMPLES / 2)
        .expect("the frame lies in the song");
    samples[index] = value;
}

#[test]
fn a_note_in_the_second_window_comes_out_with_its_pitch_and_times() {
    let (store, spec, root) = store_with_model();
    let runtime = Runtime::new();
    let model = runtime.load(&store, &spec).expect("the fake loads");

    let mut samples = vec![0.0f32; 3 * HOP_SAMPLES + 10_000];
    // Key 40 (MIDI 61) sounding over stitched frames 150..220, with
    // its onset at 150 — all inside the second window.
    for frame in 150..220 {
        say(&mut samples, 0, frame, 40, 0.8);
    }
    say(&mut samples, 1, 150, 40, 0.9);
    // A chord partner a fifth up (key 47, MIDI 68), struck together.
    for frame in 150..200 {
        say(&mut samples, 0, frame, 47, 0.7);
    }
    say(&mut samples, 1, 150, 47, 0.9);

    let transcription = transcribe_samples(
        &runtime,
        &model,
        &samples,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .expect("transcribed");
    let notes = &transcription.notes;
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!((notes[0].midi, notes[1].midi), (61, 68));
    assert!((notes[0].start_s - frame_time(150)).abs() < 1e-9);
    assert!((notes[1].start_s - frame_time(150)).abs() < 1e-9);
    assert!((notes[0].end_s - frame_time(220)).abs() < 1e-9);
    assert!((notes[1].end_s - frame_time(200)).abs() < 1e-9);
    assert!((notes[0].amplitude - 0.8).abs() < 1e-6);
    assert_eq!(transcription.model, "basic-pitch");
    assert_eq!(transcription.sha256, spec.sha256);

    // The same input gives the same answer.
    let again = transcribe_samples(
        &runtime,
        &model,
        &samples,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .expect("again");
    assert_eq!(again, transcription);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_cancelled_run_stops_rather_than_finishing() {
    let (store, spec, root) = store_with_model();
    let runtime = Runtime::new();
    let model = runtime.load(&store, &spec).expect("the fake loads");
    let result = transcribe_samples(
        &runtime,
        &model,
        &vec![0.0f32; 3 * HOP_SAMPLES],
        &mut |_, _| {},
        &AtomicBool::new(true),
    );
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(root);
}
