//! Loading stored models and running them — deterministically.
//!
//! Two rules make "same input, same output" hold on a given platform:
//! the thread count is a **constant** ([`THREADS`]), never the
//! machine's core count, so a reduction is split the same way on
//! every machine and every run; and the inference crate itself is
//! pure Rust on the CPU, with no execution provider to pick. What is
//! *not* claimed is identity across platforms — the runtime has SIMD
//! paths per architecture, and the chart generator documents the same
//! last-bit caveat.
//!
//! The interface is floats in, floats out, with names and shapes. It
//! keeps the inference crate's types out of the consumers, which is
//! what makes the runtime replaceable (ADR-0013 names the fallback).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rten::{Model, RunOptions, ThreadPool, Value, ValueOrView};
use rten_tensor::prelude::*;

use crate::error::MlError;
use crate::registry::ModelSpec;
use crate::store::ModelStore;

/// The pinned thread count. Four is a floor every machine this game
/// targets has, and a fixed split is what determinism needs; the
/// aligner (L2) measures whether 1 and 4 threads agree bit for bit and
/// drops to 1 if they do not.
pub const THREADS: usize = 4;

/// A model loaded from the store, with what identifies it.
pub struct Loaded {
    /// The registry id.
    pub id: &'static str,
    /// The file it was loaded from.
    pub path: PathBuf,
    /// The registered SHA-256 the file matched at load time. Record it
    /// in whatever the model produces.
    pub sha256: &'static str,
    model: Arc<Model>,
}

/// One input tensor: the graph's input name, a shape and the data.
#[derive(Debug, Clone, PartialEq)]
pub struct Input<'a> {
    /// The input node's name in the graph.
    pub name: &'a str,
    /// The shape, row-major.
    pub shape: Vec<usize>,
    /// The values, `shape.iter().product()` of them.
    pub data: Vec<f32>,
}

/// One output tensor, in the graph's output order.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// The output node's name in the graph, when it has one.
    pub name: String,
    /// The shape, row-major.
    pub shape: Vec<usize>,
    /// The values, row-major.
    pub data: Vec<f32>,
}

/// The session cache: models by id, on one pinned thread pool.
pub struct Runtime {
    pool: Arc<ThreadPool>,
    cache: Mutex<HashMap<&'static str, Arc<Model>>>,
}

impl Default for Runtime {
    fn default() -> Runtime {
        Runtime::new()
    }
}

impl Runtime {
    /// A runtime with [`THREADS`] threads.
    #[must_use]
    pub fn new() -> Runtime {
        Runtime {
            pool: Arc::new(ThreadPool::with_num_threads(THREADS)),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Load a model from the store, verifying it first; a model
    /// already loaded is handed out again without touching the disk.
    pub fn load(&self, store: &ModelStore, spec: &ModelSpec) -> Result<Loaded, MlError> {
        if let Some(model) = self.cached(spec.id) {
            return Ok(Loaded {
                id: spec.id,
                path: store.path(spec),
                sha256: spec.sha256,
                model,
            });
        }
        let path = store.verify(spec)?;
        let model = Model::load_file(&path).map_err(|error| MlError::Model {
            id: spec.id.to_owned(),
            reason: error.to_string(),
        })?;
        let model = Arc::new(model);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(spec.id, Arc::clone(&model));
        }
        Ok(Loaded {
            id: spec.id,
            path,
            sha256: spec.sha256,
            model,
        })
    }

    /// Whether a model is in the cache.
    #[must_use]
    pub fn is_loaded(&self, id: &str) -> bool {
        self.cached(id).is_some()
    }

    /// Drop a model from the cache. A `Loaded` still holding it keeps
    /// it alive until it is dropped too — hundreds of megabytes, so
    /// consumers should not hold on to one they are done with.
    pub fn evict(&self, id: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(id);
        }
    }

    fn cached(&self, id: &str) -> Option<Arc<Model>> {
        self.cache.lock().ok()?.get(id).cloned()
    }

    /// Run a model: every graph input by name, every graph output back
    /// in the graph's order. On the pinned pool.
    pub fn run(&self, loaded: &Loaded, inputs: &[Input<'_>]) -> Result<Vec<Output>, MlError> {
        let model = &loaded.model;
        let fail = |reason: String| MlError::Run {
            id: loaded.id.to_owned(),
            reason,
        };
        let mut bound: Vec<(rten::NodeId, ValueOrView<'static>)> = Vec::with_capacity(inputs.len());
        for input in inputs {
            let id = model
                .node_id(input.name)
                .map_err(|error| fail(format!("no input `{}`: {error}", input.name)))?;
            let value = Value::from_shape(input.shape.as_slice(), input.data.clone())
                .map_err(|error| fail(format!("input `{}`: {error}", input.name)))?;
            bound.push((id, ValueOrView::from(value)));
        }
        let output_ids = model.output_ids().to_vec();
        let mut options = RunOptions::default();
        options.thread_pool = Some(Arc::clone(&self.pool));
        let values = model
            .run(bound, &output_ids, Some(options))
            .map_err(|error| fail(error.to_string()))?;
        let mut outputs = Vec::with_capacity(values.len());
        for (id, value) in output_ids.iter().zip(values) {
            let name = model
                .node_info(*id)
                .and_then(|info| info.name().map(str::to_owned))
                .unwrap_or_default();
            let tensor = value
                .into_tensor::<f32>()
                .ok_or_else(|| fail(format!("output `{name}` is not a float tensor")))?;
            outputs.push(Output {
                name,
                shape: tensor.shape().to_vec(),
                data: tensor.into_data(),
            });
        }
        Ok(outputs)
    }
}
