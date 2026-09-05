//! The store and the runtime, end to end, against a model built in
//! this file and a web server started in this file.
//!
//! No fixture file: the ONNX graph is hand-encoded protobuf — a
//! `MatMul` of the input with a 64×64 constant — so the repository
//! carries no model, not even a toy one, and the thread pool has a
//! reduction to split. No network: the "download" comes from a
//! `TcpListener` on localhost that serves exactly what each test
//! needs — the right bytes, the wrong bytes, too many, too slowly.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use beatbyte_ml::hash::sha256_hex;
use beatbyte_ml::{Input, MlError, ModelSpec, ModelStore, Progress, Runtime, Status};

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

/// `TypeProto` for a float tensor of the given shape (0 = unknown).
fn float_type(shape: &[u64]) -> Vec<u8> {
    let mut dims = Vec::new();
    for &d in shape {
        let mut dim = Vec::new();
        field_varint(&mut dim, 1, d); // dim_value
        field_bytes(&mut dims, 1, &dim); // TensorShapeProto.dim
    }
    let mut tensor = Vec::new();
    field_varint(&mut tensor, 1, 1); // elem_type FLOAT
    field_bytes(&mut tensor, 2, &dims); // shape
    let mut ty = Vec::new();
    field_bytes(&mut ty, 1, &tensor); // TypeProto.tensor_type
    ty
}

fn value_info(name: &str, shape: &[u64]) -> Vec<u8> {
    let mut vi = Vec::new();
    field_str(&mut vi, 1, name);
    field_bytes(&mut vi, 2, &float_type(shape));
    vi
}

/// The weights: deterministic pseudo-random in (-1, 1), splitmix64.
fn weights(n: usize) -> Vec<f32> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..n)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        })
        .collect()
}

const N: usize = 64;

/// `y = MatMul(x, W)`, x: [1, N], W: [N, N] constant, y: [1, N].
fn matmul_onnx() -> Vec<u8> {
    let w = weights(N * N);
    let mut raw = Vec::with_capacity(N * N * 4);
    for value in &w {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    let mut initializer = Vec::new(); // TensorProto
    field_varint(&mut initializer, 1, N as u64); // dims
    field_varint(&mut initializer, 1, N as u64);
    field_varint(&mut initializer, 2, 1); // data_type FLOAT
    field_str(&mut initializer, 8, "W"); // name
    field_bytes(&mut initializer, 9, &raw); // raw_data

    let mut node = Vec::new(); // NodeProto
    field_str(&mut node, 1, "x");
    field_str(&mut node, 1, "W");
    field_str(&mut node, 2, "y");
    field_str(&mut node, 3, "matmul");
    field_str(&mut node, 4, "MatMul");

    let mut graph = Vec::new(); // GraphProto
    field_bytes(&mut graph, 1, &node);
    field_str(&mut graph, 2, "beatbyte-ml-test");
    field_bytes(&mut graph, 5, &initializer);
    field_bytes(&mut graph, 11, &value_info("x", &[1, N as u64]));
    field_bytes(&mut graph, 12, &value_info("y", &[1, N as u64]));

    let mut opset = Vec::new(); // OperatorSetIdProto
    field_str(&mut opset, 1, "");
    field_varint(&mut opset, 2, 13);

    let mut model = Vec::new(); // ModelProto
    field_varint(&mut model, 1, 8); // ir_version
    field_str(&mut model, 2, "beatbyte-ml tests");
    field_bytes(&mut model, 7, &graph);
    field_bytes(&mut model, 8, &opset);
    model
}

// ---- a web server of one reply -------------------------------------------

/// Serve `body` once, declaring `declared_len` bytes, at most
/// `chunk` bytes per `pause`. Returns the URL.
fn serve(body: Vec<u8>, declared_len: usize, chunk: usize, pause: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}/model.onnx", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {declared_len}\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(head.as_bytes());
        for piece in body.chunks(chunk.max(1)) {
            if stream.write_all(piece).is_err() {
                return;
            }
            let _ = stream.flush();
            std::thread::sleep(pause);
        }
    });
    url
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn spec_for(id: &str, bytes: &[u8], url: String) -> ModelSpec {
    ModelSpec {
        id: leak(id.to_owned()),
        file: "model.onnx",
        url: leak(url),
        bytes: bytes.len() as u64,
        sha256: leak(sha256_hex(bytes)),
        licence: "MIT",
        purpose: "a MatMul built by the test",
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beatbyte-ml-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn no_progress() -> impl FnMut(Progress) {
    |_| {}
}

// ---- the store --------------------------------------------------------------

#[test]
fn a_correct_download_is_installed_and_reported_in_full() {
    let model = matmul_onnx();
    let url = serve(model.clone(), model.len(), 4096, Duration::ZERO);
    let spec = spec_for("ok", &model, url);
    let store = ModelStore::at(scratch("ok"));
    assert_eq!(store.status(&spec), Status::Missing);
    assert!(matches!(
        store.verify(&spec),
        Err(MlError::NotInstalled { .. })
    ));

    let mut seen: Vec<Progress> = Vec::new();
    let path = store
        .install(&spec, &mut |p| seen.push(p), &AtomicBool::new(false))
        .expect("installs");
    assert_eq!(path, store.path(&spec));
    assert_eq!(store.status(&spec), Status::Installed);
    assert_eq!(
        std::fs::read(&path).expect("file"),
        model,
        "the bytes, exactly"
    );
    // Progress went from 0 to the full size and never past it.
    assert_eq!(seen.first().map(|p| p.done), Some(0));
    assert_eq!(seen.last().map(|p| p.done), Some(model.len() as u64));
    assert!(
        seen.iter()
            .all(|p| p.total == model.len() as u64 && p.done <= p.total)
    );
    // No `.part` left beside it.
    assert!(!path.with_extension("onnx.part").exists());

    store.remove(&spec).expect("removes");
    assert_eq!(store.status(&spec), Status::Missing);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn the_wrong_bytes_are_refused_and_nothing_is_left_behind() {
    let model = matmul_onnx();
    let mut wrong = model.clone();
    wrong[100] ^= 0xff;
    let url = serve(wrong, model.len(), 4096, Duration::ZERO);
    let spec = spec_for("wrong", &model, url);
    let store = ModelStore::at(scratch("wrong"));
    let error = store
        .install(&spec, &mut no_progress(), &AtomicBool::new(false))
        .expect_err("a wrong hash is refused");
    assert!(matches!(error, MlError::Damaged { .. }), "{error}");
    assert_eq!(store.status(&spec), Status::Missing);
    assert!(!store.path(&spec).with_extension("onnx.part").exists());
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn a_short_reply_is_damaged_and_says_how_short() {
    let model = matmul_onnx();
    let short = model[..model.len() / 2].to_vec();
    let url = serve(short.clone(), short.len(), 4096, Duration::ZERO);
    let spec = spec_for("short", &model, url);
    let store = ModelStore::at(scratch("short"));
    let error = store
        .install(&spec, &mut no_progress(), &AtomicBool::new(false))
        .expect_err("a short file is refused");
    match error {
        MlError::Damaged { actual, .. } => {
            assert!(actual.contains("of"), "names the shortfall: {actual}");
        }
        other => panic!("expected Damaged, got {other}"),
    }
    assert_eq!(store.status(&spec), Status::Missing);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn a_server_that_keeps_sending_is_stopped_at_the_registered_size() {
    let model = matmul_onnx();
    let mut long = model.clone();
    long.extend_from_slice(&[0u8; 5000]);
    let url = serve(long.clone(), long.len(), 4096, Duration::ZERO);
    let spec = spec_for("long", &model, url);
    let store = ModelStore::at(scratch("long"));
    let error = store
        .install(&spec, &mut no_progress(), &AtomicBool::new(false))
        .expect_err("more than registered is refused");
    assert!(
        matches!(error, MlError::TooLarge { expected, .. } if expected == model.len() as u64),
        "{error}"
    );
    assert_eq!(store.status(&spec), Status::Missing);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn a_cancelled_download_stops_and_keeps_nothing() {
    let model = matmul_onnx();
    // Slow enough that the cancel lands mid-stream.
    let url = serve(model.clone(), model.len(), 512, Duration::from_millis(20));
    let spec = spec_for("cancel", &model, url);
    let store = ModelStore::at(scratch("cancel"));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let mut chunks = 0usize;
    let error = store
        .install(
            &spec,
            &mut |progress| {
                chunks += 1;
                if progress.done > 0 && chunks >= 2 {
                    flag.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        )
        .expect_err("cancelled");
    assert!(matches!(error, MlError::Cancelled { .. }), "{error}");
    assert_eq!(store.status(&spec), Status::Missing);
    assert!(!store.path(&spec).with_extension("onnx.part").exists());
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn a_damaged_file_on_disk_is_seen_and_not_loaded() {
    let model = matmul_onnx();
    let spec = spec_for(
        "damaged",
        &model,
        "https://example.invalid/never".to_owned(),
    );
    let store = ModelStore::at(scratch("damaged"));
    let path = store.path(&spec);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
    std::fs::write(&path, b"not the model").expect("write");
    assert!(matches!(store.status(&spec), Status::Damaged { .. }));
    assert!(matches!(store.verify(&spec), Err(MlError::Damaged { .. })));
    let runtime = Runtime::new();
    assert!(matches!(
        runtime.load(&store, &spec),
        Err(MlError::Damaged { .. })
    ));
    let _ = std::fs::remove_dir_all(store.root());
}

// ---- the runtime ------------------------------------------------------------

fn installed(name: &str) -> (ModelStore, ModelSpec) {
    let model = matmul_onnx();
    let url = serve(model.clone(), model.len(), 65_536, Duration::ZERO);
    let spec = spec_for(name, &model, url);
    let store = ModelStore::at(scratch(name));
    store
        .install(&spec, &mut no_progress(), &AtomicBool::new(false))
        .expect("installs");
    (store, spec)
}

fn ramp() -> Vec<f32> {
    (0..N).map(|i| i as f32 / N as f32 - 0.5).collect()
}

#[test]
fn a_stored_model_loads_runs_and_agrees_with_arithmetic() {
    let (store, spec) = installed("run");
    let runtime = Runtime::new();
    let loaded = runtime.load(&store, &spec).expect("loads");
    assert_eq!(loaded.sha256, spec.sha256);
    let x = ramp();
    let outputs = runtime
        .run(
            &loaded,
            &[Input {
                name: "x",
                shape: vec![1, N],
                data: x.clone(),
            }],
        )
        .expect("runs");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].name, "y");
    assert_eq!(outputs[0].shape, vec![1, N]);
    // Against a plain loop: the runtime's summation order may differ
    // in the last bits, so a tolerance — bit-identity is the NEXT
    // test's business, between the runtime and itself.
    let w = weights(N * N);
    for (j, got) in outputs[0].data.iter().enumerate() {
        let want: f32 = (0..N).map(|i| x[i] * w[i * N + j]).sum();
        assert!((got - want).abs() < 1e-4, "y[{j}]: {got} vs {want}");
    }
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn the_same_input_gives_the_same_bits_run_after_run() {
    let (store, spec) = installed("determinism");
    let runtime = Runtime::new();
    let loaded = runtime.load(&store, &spec).expect("loads");
    let input = || {
        vec![Input {
            name: "x",
            shape: vec![1, N],
            data: ramp(),
        }]
    };
    let first = runtime.run(&loaded, &input()).expect("runs");
    for _ in 0..20 {
        let again = runtime.run(&loaded, &input()).expect("runs");
        assert_eq!(
            first[0]
                .data
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            again[0]
                .data
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            "a run differed from the first, bit for bit"
        );
    }
    // And a second runtime — a second pinned pool — agrees too.
    let other = Runtime::new();
    let elsewhere = other.load(&store, &spec).expect("loads");
    let theirs = other.run(&elsewhere, &input()).expect("runs");
    assert_eq!(
        first[0]
            .data
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        theirs[0]
            .data
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn the_cache_hands_out_the_loaded_model_until_evicted() {
    let (store, spec) = installed("cache");
    let runtime = Runtime::new();
    assert!(!runtime.is_loaded(spec.id));
    let first = runtime.load(&store, &spec).expect("loads");
    assert!(runtime.is_loaded(spec.id));
    // Damage the file on disk: a cached model is served without
    // touching it, an evicted one is verified again and refused.
    std::fs::write(store.path(&spec), b"gone").expect("overwrite");
    let cached = runtime.load(&store, &spec).expect("from the cache");
    assert_eq!(cached.sha256, first.sha256);
    runtime.evict(spec.id);
    assert!(!runtime.is_loaded(spec.id));
    assert!(matches!(
        runtime.load(&store, &spec),
        Err(MlError::Damaged { .. })
    ));
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn a_wrong_input_name_or_shape_is_an_error_not_a_panic() {
    let (store, spec) = installed("inputs");
    let runtime = Runtime::new();
    let loaded = runtime.load(&store, &spec).expect("loads");
    let wrong_name = runtime.run(
        &loaded,
        &[Input {
            name: "nope",
            shape: vec![1, N],
            data: ramp(),
        }],
    );
    assert!(
        matches!(wrong_name, Err(MlError::Run { .. })),
        "{wrong_name:?}"
    );
    let wrong_shape = runtime.run(
        &loaded,
        &[Input {
            name: "x",
            shape: vec![1, N + 1],
            data: ramp(),
        }],
    );
    assert!(
        matches!(wrong_shape, Err(MlError::Run { .. })),
        "{wrong_shape:?}"
    );
    let _ = std::fs::remove_dir_all(store.root());
}
