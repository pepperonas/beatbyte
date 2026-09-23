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

/// Documentation examples across the workspace: the fenced blocks in
/// doc comments that rustdoc compiles and runs as tests.
///
/// Two things this has to get right, and neither is obvious. A
/// ```` ```text ```` block is prose and is not run — counting it
/// would inflate the total by one per explanatory diagram. And a
/// block's CLOSING fence is bare, so a naive count of ```` ``` ````
/// reads every closing fence as another example; the state has to be
/// tracked rather than the occurrences counted.
fn doc_examples() -> usize {
    let mut files = Vec::new();
    for dir in ["crates", "apps"] {
        rust_files(&repo().join(dir), &mut files);
    }
    let mut count = 0;
    for path in &files {
        // Only a library's own sources carry doc tests; a `//!` at the
        // top of an integration test is not compiled as one.
        if !path.components().any(|c| c.as_os_str() == "src") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let mut open = false;
        for line in text.lines() {
            let trimmed = line.trim_start();
            let Some(doc) = trimmed
                .strip_prefix("///")
                .or_else(|| trimmed.strip_prefix("//!"))
            else {
                continue;
            };
            let Some(info) = doc.trim_start().strip_prefix("```") else {
                continue;
            };
            if open {
                open = false;
                continue;
            }
            open = true;
            let info = info.trim();
            if info.is_empty() || info.starts_with("rust") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn the_documentation_example_counter_tells_prose_from_tests() {
    // The counter decides a number the README is held to, so it gets
    // its own proof rather than being trusted: a `text` block is
    // prose, a bare one is a test, and a closing fence is neither.
    assert!(
        doc_examples() >= 2,
        "the workspace has at least the chart crate's example and the \
         pitch conversion's; found {}",
        doc_examples()
    );
}

#[test]
fn the_readme_total_is_the_sum_of_its_parts() {
    // The command line in the README quotes what `cargo test` prints,
    // which is every test function PLUS the documentation examples it
    // compiles and runs, and the ignored ones are in that number too.
    // So the total is the table's sum plus however many doc examples
    // the workspace carries — counted, because the first version of
    // this test hard-coded "one" and the second example duly broke it.
    let doc_examples = doc_examples();
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
fn the_catalogue_document_lists_every_achievement() {
    // `docs/achievements.md` is the engineering reference for the
    // three hundred achievements and the field each rule reads. It is
    // written from the catalogue and read by people; without this it
    // would be a snapshot of whatever the catalogue was on the day
    // somebody last looked.
    let code = read("crates/beatbyte-core/src/achievements.rs");
    let doc = read("docs/achievements.md");
    let ids: Vec<&str> = code
        .lines()
        .filter_map(|line| line.trim().strip_prefix("id: \""))
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert_eq!(
        ids.len(),
        300,
        "the catalogue is meant to hold exactly three hundred achievements"
    );
    let missing: Vec<&&str> = ids
        .iter()
        .filter(|id| !doc.contains(&format!("| `{id}` |")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/achievements.md does not list: {missing:?}"
    );
    // And nothing the catalogue has dropped may linger in the
    // document: a row for an achievement nobody can earn is worse
    // than a missing one, because it reads as a promise.
    let rows = doc.lines().filter(|line| line.starts_with("| `")).count();
    assert_eq!(
        rows,
        ids.len(),
        "the document has rows the catalogue does not"
    );
    // The count the document states about itself.
    let hidden = code.matches("hidden: true").count();
    assert!(
        doc.contains(&format!(
            "{} achievements, {hidden} of them hidden.",
            ids.len()
        )),
        "the document's own summary line disagrees with the catalogue \
         ({} achievements, {hidden} hidden)",
        ids.len()
    );
}

#[test]
fn every_screen_that_reads_the_play_log_waits_for_it_to_be_reloaded() {
    // Three screens read `PlayHistory` on the same state entry that
    // reloads it. Two systems in one schedule, one writing what the
    // other reads, are ordered by nothing — the achievements screen
    // could draw "3 / 10" from a copy that predates the run the
    // player just finished. `history::HistoryReloaded` is the set
    // they order behind, and this is what stops it being quietly
    // dropped by a later edit.
    let order = "after(crate::history::HistoryReloaded)";
    for file in [
        "crates/beatbyte-game/src/achievements_ui.rs",
        "crates/beatbyte-game/src/achievements.rs",
        "crates/beatbyte-game/src/stats_ui.rs",
        "crates/beatbyte-game/src/players_ui.rs",
    ] {
        assert!(
            read(file).contains(order),
            "{file} reads the play log on a state entry without ordering behind the reload"
        );
    }
    // And the set has to be on every reload, or ordering behind it
    // means nothing for the screen whose reload was left out.
    let history = read("crates/beatbyte-game/src/history.rs");
    assert_eq!(
        history
            .matches("reload_history.in_set(HistoryReloaded)")
            .count(),
        3,
        "not every screen's reload is in the set"
    );
}

#[test]
fn the_decision_record_counts_the_tests_it_claims() {
    // ADR-0017's Verification section states how many tests back the
    // decision. Those two numbers went stale TWICE inside one
    // session — once while the feature was being finished, once
    // while five rounds of fixes were added on top — and nothing
    // caught either. A record of verification that quietly stops
    // being true is worse than one that gives no number at all.
    let adr = read("docs/decisions/ADR-0017-achievements-derived-not-counted.md");
    let claimed = |after: &str| -> usize {
        let at = adr
            .find(after)
            .unwrap_or_else(|| panic!("`{after}` is no longer in the ADR"));
        adr[at + after.len()..]
            .split_whitespace()
            .next()
            .and_then(|word| word.parse().ok())
            .unwrap_or_else(|| panic!("no number after `{after}`"))
    };
    let counted = |files: &[&str]| -> usize {
        files
            .iter()
            .map(|file| read(file).matches("#[test]").count())
            .sum()
    };
    assert_eq!(
        claimed("Pure logic: "),
        counted(&["crates/beatbyte-core/src/achievements.rs"]),
        "the ADR's core test count is stale"
    );
    assert_eq!(
        claimed("Store and screen: "),
        counted(&[
            "crates/beatbyte-game/src/achievements.rs",
            "crates/beatbyte-game/src/achievements_ui.rs",
        ]),
        "the ADR's game test count is stale"
    );
}

#[test]
fn the_roadmap_and_the_readme_agree_on_the_test_total() {
    // The roadmap's entry quotes the suite size as evidence. The
    // README's badge quotes the same number and IS checked against
    // the code; this ties the second copy to the first rather than
    // leaving it to be remembered.
    let readme = readme_test_table().iter().map(|(_, n)| n).sum::<usize>() + doc_examples();
    let roadmap = read("docs/ROADMAP.md");
    let quoted = format!("*Verified: {readme} tests");
    assert!(
        roadmap.contains(&quoted),
        "the roadmap's newest entry does not quote {readme} tests"
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
    let total: usize = readme_test_table().iter().map(|(_, n)| n).sum::<usize>() + doc_examples();
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
fn the_lyrics_evaluation_quotes_the_floor_it_measured() {
    // `docs/lyrics/evaluation.md` places the legibility floor from a
    // corpus measurement and prints the number in a table. A document
    // that quotes a constant goes wrong the moment the constant
    // moves, and this one would go wrong quietly: the prose about
    // "where the contrast is widest" reads just as well beside the
    // wrong figure.
    let floor = beatbyte_lyrics::gate::GateConfig::default().min_letters_per_s;
    let doc = read("docs/lyrics/evaluation.md");
    assert!(
        doc.contains(&format!("| **{floor:.1}** |")),
        "the evaluation's table does not mark {floor:.1} as the chosen floor"
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

    // The rock meter's figures, the same way.
    for (name, value) in [
        ("start", config.meter_start),
        ("per hit", config.meter_per_hit),
        ("per miss", config.meter_per_miss),
        ("per overstrum", config.meter_per_overstrum),
    ] {
        // Exact, not rounded: a 1.5 % constant must not pass as "2%".
        let percent = value * 100.0;
        assert!(
            (percent - percent.round()).abs() < 1e-9,
            "the rock meter's {name} is {percent}%, which this document \
             cannot state as a whole number — choose one it can"
        );
        let percent = percent.round() as u32;
        assert!(
            rules.contains(&format!("**{percent}%**")),
            "the rules do not state the rock meter's {name} of {percent}%"
        );
    }
    assert!(
        rules.contains(if config.fail_when_empty {
            "No Fail** is off by default"
        } else {
            "No Fail** is on by default"
        }),
        "the rules must state the No Fail default the code ships"
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

/// The README may not claim the game never uses the network while a
/// crate contains code that does.
///
/// This one is written from a mistake of mine: the lyrics lookup was
/// added, shipped, and documented — and the badge went on saying
/// `network-never` for several releases, because nothing about a
/// stale badge looks stale. A privacy claim is the last claim that
/// should be maintained by hand, so it is counted instead.
#[test]
fn the_network_claim_matches_what_the_code_actually_does() {
    let readme = read("README.md");
    // Every place a request could originate: the crates that are
    // allowed to talk to anything at all.
    let reaches_out = [
        "crates/beatbyte-game/src/lyrics_fetch.rs",
        "crates/beatbyte-game/src/room_stage.rs",
        "crates/beatbyte-ml/src/store.rs",
    ]
    .iter()
    .any(|path| repo().join(path).is_file());
    // A second outbound path arrived with Room Stage, and the claim
    // has to name it too — the badge said "lyrics lookup only" the
    // moment that stopped being true.
    let room_stage = repo()
        .join("crates/beatbyte-game/src/room_stage.rs")
        .is_file();
    // The third: the ML model store downloads a model on request
    // (ADR-0013). Behind a feature, one time, to a pinned URL — and
    // the section has to say all three of those things.
    let model_store = repo().join("crates/beatbyte-ml/src/store.rs").is_file();

    if reaches_out {
        assert!(
            !readme.contains("network-never"),
            "the README claims the game never uses the network, but \
             an outbound request exists in the tree"
        );
        // And it has to say what goes out, not merely drop the claim.
        assert!(
            readme.contains("### What leaves your machine"),
            "a game that makes a request owes the reader a section \
             saying which one, and when"
        );
        assert!(
            readme.contains("lrclib.net"),
            "the section must name the service that is contacted"
        );
        if room_stage {
            assert!(
                readme.contains("Room Stage"),
                "Room Stage posts to the local network; the section \
                 that lists what leaves the machine must say so"
            );
            assert!(
                !readme.contains("gameplay never touches the network"),
                "with Room Stage in the tree, gameplay CAN touch the \
                 local network — the claim needs its exception"
            );
            assert!(
                !readme.contains("lyrics%20lookup%20only"),
                "the badge still says the lyrics lookup is the only \
                 request the game makes"
            );
        }
        // The fourth thing is not a request at all, but it is the
        // one a reader worries about most: an open microphone. The
        // section has to say it is read and never leaves.
        if repo().join("crates/beatbyte-audio/src/listen.rs").is_file() {
            let section = readme
                .split("### What leaves your machine")
                .nth(1)
                .unwrap_or_default();
            assert!(
                section.contains("microphone"),
                "the game opens the audio input; the section must say so"
            );
            assert!(
                section.contains("never written, kept or sent"),
                "and say what becomes of the samples"
            );
        }
        if model_store {
            let section = readme
                .split("### What leaves your machine")
                .nth(1)
                .unwrap_or_default();
            assert!(
                section.contains("models install"),
                "the section must name the command that downloads a model"
            );
            assert!(
                section.contains("--features ml"),
                "the section must say the download code exists only behind the feature"
            );
            assert!(
                section.contains("SHA-256"),
                "the section must say a download is verified against a pinned hash"
            );
        }
    } else {
        assert!(
            readme.contains("network-never"),
            "nothing in the tree reaches out; say so"
        );
    }
}
