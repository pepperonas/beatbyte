//! Tests that fail when the documentation stops matching the code.
//!
//! Every number in the README was written by hand, and every one of
//! them has drifted at least once — the test count was corrected
//! twice in a single day and was stale again by the evening. Prose
//! can be reviewed; a number cannot, because nothing about a wrong
//! one looks wrong.
//!
//! So the numbers are checked here instead. These tests read the
//! repository as data: they count what is actually in the source and
//! compare it against what the documents claim. They live in the
//! launcher crate because it is the only workspace member whose tests
//! have no reason to care about anything else, and the repository
//! root is two levels up from it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The repository root, from this crate's manifest directory.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("apps/beatbyte sits two levels below the repository root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

/// Every `.rs` file under a directory, recursively.
fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` holds generated code and would dwarf the count.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_files(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push(path);
        }
    }
}

/// Test functions in one crate: `#[test]` attributes in its sources.
///
/// This counts the same thing `cargo test` reports, with one known
/// exception documented in [`the_readme_total_is_the_sum_of_its_parts`].
fn test_count(crate_name: &str) -> usize {
    let mut files = Vec::new();
    // The five libraries live under `crates/`; the launcher — which
    // owns these consistency tests — lives under `apps/`.
    let root = repo().join("crates").join(crate_name);
    let root = if root.is_dir() {
        root
    } else {
        repo().join("apps").join(crate_name)
    };
    rust_files(&root, &mut files);
    files
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .map(|text| {
            // Whole lines only. A substring search counts prose and
            // string literals too — this very file mentions the
            // attribute twice while explaining how it counts it, and
            // the first version duly counted itself as two extra
            // tests. rustfmt always puts the attribute on its own
            // line, so this is exact rather than merely closer.
            text.lines().filter(|line| line.trim() == "#[test]").count()
        })
        .sum()
}

/// The per-crate rows of the README's testing table, as
/// `(crate, claimed count)`.
fn readme_test_table() -> Vec<(String, usize)> {
    read("README.md")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("| `beatbyte")?;
            let (suffix, rest) = rest.split_once('`')?;
            let claimed = rest.trim_start_matches(" |").trim().split('|').next()?;
            Some((format!("beatbyte{suffix}"), claimed.trim().parse().ok()?))
        })
        .collect()
}

#[test]
fn the_readme_test_table_matches_the_code() {
    let table = readme_test_table();
    assert!(
        table.len() >= 6,
        "the README's testing table lost its rows: found {table:?}"
    );
    for (crate_name, claimed) in table {
        let actual = test_count(&crate_name);
        assert_eq!(
            claimed, actual,
            "README says {crate_name} has {claimed} tests; it has {actual}. \
             Update the table rather than this test."
        );
    }
}

#[test]
fn the_readme_total_is_the_sum_of_its_parts() {
    // The command line in the README quotes what `cargo test` prints,
    // which is every test function PLUS the documentation examples it
    // compiles and runs. There is exactly one of those, in
    // beatbyte-chart's crate docs, so the total is the table's sum
    // plus one. If a second doc example is added this fails, which is
    // the right moment to notice the README needs a word about it.
    let doc_examples = read("crates/beatbyte-chart/src/lib.rs")
        .matches("```")
        .count()
        / 2;
    let sum: usize = readme_test_table().iter().map(|(_, n)| n).sum();
    let readme = read("README.md");
    let claimed: usize = readme
        .lines()
        .find_map(|line| {
            let (_, rest) = line.split_once("cargo test --workspace")?;
            rest.split_whitespace()
                .find_map(|word| word.parse::<usize>().ok())
        })
        .expect("the README quotes a total after `cargo test --workspace`");
    assert_eq!(
        claimed,
        sum + doc_examples,
        "README claims {claimed} tests in total; the table sums to {sum} \
         plus {doc_examples} documentation example(s)"
    );
}

#[test]
fn the_version_has_a_changelog_section() {
    // The rule this project runs on: the version in the manifest moves
    // with every user-visible change, and a version nobody wrote down
    // is a version nobody can explain.
    let version = read("Cargo.toml")
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.split('"').next())
                .map(ToOwned::to_owned)
        })
        .expect("the workspace manifest declares a version");
    let changelog = read("CHANGELOG.md");
    let heading = format!("## [{version}]");
    assert!(
        changelog.contains(&heading),
        "Cargo.toml is at {version} but CHANGELOG.md has no `{heading}` section"
    );
    // And it must be the NEWEST section, or the manifest is behind.
    let newest = changelog
        .lines()
        .find(|line| line.starts_with("## ["))
        .expect("the changelog has at least one version section");
    assert!(
        newest.starts_with(&heading),
        "the newest changelog section is `{newest}`, but the manifest says {version}"
    );
}

#[test]
fn every_internal_dependency_moves_with_the_version() {
    // The workspace crates are pinned to each other by exact version.
    // Bumping the workspace version without them leaves a manifest
    // that cannot resolve — caught here rather than at publish time.
    let manifest = read("Cargo.toml");
    let version = manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.split('"').next())
        })
        .expect("the workspace manifest declares a version");
    for line in manifest.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("beatbyte-") || !trimmed.contains("version = ") {
            continue;
        }
        assert!(
            trimmed.contains(&format!("version = \"{version}\"")),
            "internal dependency is not at {version}: {trimmed}"
        );
    }
}

#[test]
fn every_local_link_in_the_readme_resolves() {
    // A broken link is the cheapest documentation bug to make and the
    // most annoying to meet: it costs nothing to write and it wastes a
    // reader's time completely.
    let readme = read("README.md");
    let mut missing = Vec::new();
    for capture in readme.split("](").skip(1) {
        let Some(target) = capture.split(')').next() else {
            continue;
        };
        // External links and in-page anchors are somebody else's
        // problem; only repository paths are checkable here.
        if target.starts_with("http") || target.starts_with('#') || target.is_empty() {
            continue;
        }
        let path = target.split('#').next().unwrap_or(target);
        if !repo().join(path).exists() {
            missing.push(path.to_owned());
        }
    }
    assert!(
        missing.is_empty(),
        "README links to missing paths: {missing:?}"
    );
}

#[test]
fn the_adr_index_lists_every_decision_record() {
    // An index that quietly omits a record is worse than no index:
    // the reader believes they have seen everything.
    let index = read("docs/decisions/README.md");
    let mut on_disk = BTreeSet::new();
    for entry in fs::read_dir(repo().join("docs/decisions")).expect("decisions directory") {
        let name = entry.expect("directory entry").file_name();
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("ADR-") {
            on_disk.insert(name);
        }
    }
    assert!(!on_disk.is_empty(), "no ADRs found on disk");
    for name in &on_disk {
        assert!(
            index.contains(name.as_str()),
            "docs/decisions/README.md does not list {name}"
        );
    }
}

#[test]
fn the_harness_reference_documents_every_switch() {
    // Twelve of the fourteen environment variables were documented
    // nowhere but the source once. A harness nobody can find is a
    // harness nobody runs.
    let mut used = BTreeSet::new();
    for dir in ["crates", "apps"] {
        let mut files = Vec::new();
        rust_files(&repo().join(dir), &mut files);
        for text in files.iter().filter_map(|p| fs::read_to_string(p).ok()) {
            let mut rest = text.as_str();
            while let Some(at) = rest.find("BEATBYTE_") {
                let tail = &rest[at..];
                let end = tail
                    .find(|c: char| !c.is_ascii_uppercase() && c != '_' && !c.is_ascii_digit())
                    .unwrap_or(tail.len());
                used.insert(tail[..end].to_owned());
                rest = &tail[end..];
            }
        }
    }
    let doc = read("docs/development/harness.md");
    let undocumented: Vec<_> = used
        .iter()
        .filter(|name| !doc.contains(name.as_str()))
        .collect();
    assert!(
        undocumented.is_empty(),
        "these switches exist in the source but not in the harness reference: {undocumented:?}"
    );
}

#[test]
fn checkable_badges_state_the_truth() {
    let readme = read("README.md");
    let manifest = read("Cargo.toml");

    // Workspace member count.
    let members = manifest
        .lines()
        .skip_while(|line| !line.starts_with("members"))
        .take_while(|line| !line.starts_with(']'))
        .filter(|line| line.contains('"'))
        .count();
    assert!(
        readme.contains(&format!("workspace-{members}%20crates")),
        "the workspace badge does not say {members} crates"
    );

    // Minimum supported Rust version, which the manifest declares.
    let msrv = manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("rust-version = \"")
                .and_then(|rest| rest.split('"').next())
        })
        .expect("the manifest declares rust-version");
    assert!(
        readme.contains(&format!("MSRV-{msrv}")),
        "the MSRV badge does not say {msrv}"
    );

    // The tests badge quotes the same total the command line does
    // (table sum + doc examples). It sat at 313 while the suite was
    // at 422 — nothing about a wrong number looks wrong, so it is
    // enforced now like the rest.
    let doc_examples = read("crates/beatbyte-chart/src/lib.rs")
        .matches("```")
        .count()
        / 2;
    let total: usize = readme_test_table().iter().map(|(_, n)| n).sum::<usize>() + doc_examples;
    assert!(
        readme.contains(&format!("tests-{total}%20passing")),
        "the tests badge does not say {total} passing"
    );

    // Harness switches: the badge counts the reference's table rows.
    let switches = read("docs/development/harness.md")
        .lines()
        .filter(|line| line.starts_with("| `BEATBYTE_"))
        .count();
    assert!(
        readme.contains(&format!("harness%20switches-{switches}")),
        "the harness badge does not say {switches} switches"
    );

    // Decision records.
    let adrs = fs::read_dir(repo().join("docs/decisions"))
        .expect("decisions directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("ADR-"))
        .count();
    assert!(
        readme.contains(&format!("decisions-{adrs}%20ADRs")),
        "the ADR badge does not say {adrs}"
    );
}

#[test]
fn no_document_still_promises_an_unreleased_section() {
    // The changelog once had an `[Unreleased]` heading, and three
    // documents told the reader to file their entry there. When the
    // versioning rule changed, all three kept saying it — which is
    // precisely the kind of quiet contradiction this file exists to
    // prevent, so it is now checked rather than remembered.
    let mut offenders = Vec::new();
    for doc in [
        "README.md",
        "CONTRIBUTING.md",
        "CLAUDE.md",
        "CHANGELOG.md",
        "docs/releases/process.md",
    ] {
        let text = read(doc);
        for (number, line) in text.lines().enumerate() {
            // The rule is about instructions, not history: a line that
            // explains the section is GONE is exactly what should be
            // written.
            if line.contains("[Unreleased]") && !line.contains("no `[Unreleased]`") {
                offenders.push(format!("{doc}:{}", number + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these lines still refer to an [Unreleased] section: {offenders:?}"
    );
}

#[test]
fn the_rules_document_quotes_the_real_numbers() {
    // `docs/gameplay/rules.md` states the multiplier thresholds, the
    // meter a phrase awards and the activation threshold as figures.
    // They live in `ScoreConfig`, and a document that quotes a
    // constant is a document that goes wrong the moment the constant
    // moves — silently, because the prose around it still reads well.
    let config = beatbyte_core::ScoreConfig::default();
    let rules = read("docs/gameplay/rules.md");

    let per_level = config.streak_per_level;
    for level in 2..=config.max_multiplier {
        let threshold = per_level * (level - 1);
        assert!(
            rules.contains(&format!("×{level} at {threshold}"))
                || rules.contains(&format!("×{level} at streak {threshold}")),
            "the rules do not state ×{level} at streak {threshold}"
        );
    }

    let phrase_percent = (config.hype_per_phrase * 100.0).round() as u32;
    assert!(
        rules.contains(&format!("{phrase_percent}%")),
        "the rules do not state that a phrase awards {phrase_percent}% meter"
    );

    let activation_percent = (config.hype_activation_threshold * 100.0).round() as u32;
    assert!(
        rules.contains(&format!("{activation_percent}%")),
        "the rules do not state the {activation_percent}% activation threshold"
    );

    assert!(
        rules.contains(&format!("×{}", config.max_multiplier)),
        "the rules do not state the ×{} cap",
        config.max_multiplier
    );
}

#[test]
fn the_design_workflow_only_names_real_subcommands() {
    // The workflow document walks through CLI invocations; a renamed
    // or removed subcommand would leave it teaching commands that do
    // not exist. The Command enum is the truth.
    let main_rs = read("crates/beatbyte-cli/src/main.rs");
    let mut subcommands = BTreeSet::new();
    let mut in_enum = false;
    for line in main_rs.lines() {
        if line.starts_with("enum Command {") {
            in_enum = true;
            continue;
        }
        if in_enum {
            if line.starts_with('}') {
                break;
            }
            let trimmed = line.trim();
            // A variant line: an identifier followed by ` {`.
            if let Some(name) = trimmed.strip_suffix(" {")
                && name.chars().all(char::is_alphanumeric)
                && name.chars().next().is_some_and(char::is_uppercase)
            {
                subcommands.insert(name.to_lowercase());
            }
        }
    }
    assert!(
        subcommands.len() >= 5,
        "the Command enum was not found where expected"
    );
    let doc = read("docs/workflow/design-session.md");
    for line in doc.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("beatbyte-cli ") {
            rest = &rest[at + "beatbyte-cli ".len()..];
            let word: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !word.is_empty() {
                assert!(
                    subcommands.contains(&word),
                    "the workflow doc invokes `beatbyte-cli {word}`, which does not exist \
                     (real subcommands: {subcommands:?})"
                );
            }
        }
    }
}
