//! `beatbyte-cli models …` — the explicit way to fetch, check and
//! remove local ML models (ADR-0013). Behind the `ml` feature.
//!
//! This is the only place in the CLI that can reach the network, and
//! it does so only for `install`, only to the URL the registry pins,
//! and only when asked.

use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use beatbyte_ml::{ModelStore, Progress, REGISTRY, Status, spec};

/// Which store the commands work on.
fn store() -> Option<ModelStore> {
    let store = ModelStore::default_location();
    if store.is_none() {
        eprintln!("no config directory on this platform; models cannot be stored");
    }
    store
}

/// `models list`: every model the build knows, with its state.
pub fn list() -> ExitCode {
    let Some(store) = store() else {
        return ExitCode::from(2);
    };
    println!("store: {}", store.root().display());
    if REGISTRY.is_empty() {
        println!("(this build registers no models yet)");
        return ExitCode::SUCCESS;
    }
    for model in REGISTRY {
        let state = match store.status(model) {
            Status::Installed => "installed".to_owned(),
            Status::Missing => "not installed".to_owned(),
            Status::Damaged { .. } => "DAMAGED — install again".to_owned(),
        };
        println!(
            "{:<28} {:>9} MB  {:<12} {}  [{}]",
            model.id,
            model.bytes / 1_000_000,
            model.licence,
            state,
            model.purpose
        );
    }
    ExitCode::SUCCESS
}

/// `models install <id>`: fetch and verify one model.
pub fn install(id: &str) -> ExitCode {
    let (Some(store), Some(model)) = (store(), spec(id)) else {
        if spec(id).is_none() {
            eprintln!("unknown model `{id}` — `models list` names the ones this build knows");
        }
        return ExitCode::from(2);
    };
    if store.status(model) == Status::Installed {
        println!("`{id}` is already installed and intact");
        return ExitCode::SUCCESS;
    }
    println!(
        "downloading `{id}` ({} MB, {}) from {}",
        model.bytes / 1_000_000,
        model.licence,
        model.url
    );
    let mut last_percent = u64::MAX;
    let mut progress = |p: Progress| {
        let percent = (p.done * 100).checked_div(p.total).unwrap_or(100);
        if percent != last_percent && percent.is_multiple_of(5) {
            eprintln!("  {percent:>3} %");
            last_percent = percent;
        }
    };
    match store.install(model, &mut progress, &AtomicBool::new(false)) {
        Ok(path) => {
            println!("installed `{id}` at {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

/// `models verify <id>`: re-hash an installed model.
pub fn verify(id: &str) -> ExitCode {
    let (Some(store), Some(model)) = (store(), spec(id)) else {
        if spec(id).is_none() {
            eprintln!("unknown model `{id}`");
        }
        return ExitCode::from(2);
    };
    match store.verify(model) {
        Ok(path) => {
            println!("`{id}` is intact: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

/// `models remove <id>`: delete an installed model.
pub fn remove(id: &str) -> ExitCode {
    let (Some(store), Some(model)) = (store(), spec(id)) else {
        if spec(id).is_none() {
            eprintln!("unknown model `{id}`");
        }
        return ExitCode::from(2);
    };
    match store.remove(model) {
        Ok(()) => {
            println!("removed `{id}`");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
