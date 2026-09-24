//! The gate that keeps the telemetry store EVIDENCE.
//!
//! ADR-0018 gave the store one hard property: analytics produce
//! evidence, and nothing in the game changes a chart, a score, a
//! difficulty or a setting because of what the store says. Until
//! v0.19 that was guaranteed structurally — the game never read the
//! store at all, so nothing could act on it.
//!
//! The Player Analytics work changed that deliberately (design spec
//! 2026-09-23, "Store read policy — A"): the statistics screen opens
//! `telemetry.db` **read-only** to show the player what happened. The
//! property still holds, but it is no longer guaranteed by there
//! being no reader — it is now a discipline, and a discipline that
//! only lives in prose is one the next change erodes without noticing.
//!
//! So it is pinned here, as three facts about the source:
//!
//! 1. exactly one module reads the store back,
//! 2. that module never takes a writable handle,
//! 3. exactly one module opens a writable one.
//!
//! ⚠️ The checks run over the source with comments REMOVED. The
//! documents of this house quote their own rules verbatim — this very
//! file names every pattern it forbids — and a check that searched
//! the raw text would find its own prose and pass or fail for the
//! wrong reason.

use std::fs;
use std::path::{Path, PathBuf};

/// The repository root, from this test's own location.
fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("apps/beatbyte lives two levels below the root")
        .to_path_buf()
}

/// Every `.rs` file under a directory, recursively.
fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push(path);
        }
    }
}

/// The source with its comments taken out, and nothing else changed.
///
/// ⚠️ String literals are tracked, because they are where a naive
/// stripper goes wrong: `"https://example.com"` carries a `//` that
/// is not a comment, and cutting from there to the end of the line
/// would silently swallow whatever else that line said — including,
/// one day, the very call this gate exists to find.
fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;
    // Copy a quoted run whole, honouring backslash escapes.
    let copy_quoted = |chars: &[char], out: &mut String, i: &mut usize, quote: char| {
        out.push(chars[*i]);
        *i += 1;
        while *i < chars.len() {
            let c = chars[*i];
            out.push(c);
            *i += 1;
            if c == '\\' {
                if let Some(escaped) = chars.get(*i) {
                    out.push(*escaped);
                    *i += 1;
                }
            } else if c == quote {
                break;
            }
        }
    };
    while i < chars.len() {
        match chars[i] {
            '"' => copy_quoted(&chars, &mut out, &mut i, '"'),
            '\'' if opens_a_char(&chars, i) => copy_quoted(&chars, &mut out, &mut i, '\''),
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                let mut depth = 1usize;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        if chars[i] == '\n' {
                            out.push('\n');
                        }
                        i += 1;
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// Whether the quote at `at` opens a character literal rather than a
/// lifetime (`&'a str`) or a loop label (`'outer:`).
///
/// ⚠️ Guessing this from the NEXT character alone is what the first
/// version did, and it got the CLOSING quote of `'a'` wrong: it saw
/// `';'` after it, decided a literal had begun, and spent the rest of
/// the file believing it was inside one — so every comment that
/// followed was read as code. Nothing was swallowed (each character
/// is copied either way), which is why it looked fine; what it cost
/// was the stripping, and the gate would have raised a false alarm
/// about this house's own prose. Two characters of lookahead settle
/// it exactly.
fn opens_a_char(chars: &[char], at: usize) -> bool {
    match chars.get(at + 1) {
        // An escape is always a literal: '\n', '\'', '\u{1F600}'.
        Some('\\') => true,
        // One character and then the closing quote.
        Some(_) => chars.get(at + 2) == Some(&'\''),
        None => false,
    }
}

/// Every game-crate source file, as code, with its path.
fn game_sources() -> Vec<(String, String)> {
    let dir = repo().join("crates/beatbyte-game/src");
    let mut files = Vec::new();
    rust_files(&dir, &mut files);
    assert!(
        !files.is_empty(),
        "no sources found under {}",
        dir.display()
    );
    files
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let code = code_only(&fs::read_to_string(&path).expect("reads"));
            (name, code)
        })
        .collect()
}

#[test]
fn the_store_is_read_back_in_exactly_one_place() {
    // One reader is what keeps "evidence, never a change" checkable.
    // A second one is not forbidden because two is worse than one —
    // it is forbidden because the moment a reader appears somewhere
    // that DECIDES something, the property is gone and nothing says
    // so.
    let readers: Vec<String> = game_sources()
        .into_iter()
        .filter(|(_, code)| code.contains("open_readonly") || code.contains("telemetry::analytics"))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        readers,
        vec!["stats_ui.rs".to_owned()],
        "the statistics screen is the only place that may read the \
         store back; a new reader needs ADR-0018 to say so first"
    );
}

#[test]
fn the_reader_never_takes_a_writable_handle() {
    // The screen reads a file the game is writing to. A writable
    // handle would let a display bug migrate or damage the record it
    // is displaying — and `open_readonly` deliberately does not run
    // the migrations that `open` does.
    let (_, code) = game_sources()
        .into_iter()
        .find(|(name, _)| name == "stats_ui.rs")
        .expect("the statistics screen");
    assert!(
        !code.contains("Store::open("),
        "the statistics screen opened the store for writing"
    );
}

#[test]
fn only_the_writer_module_opens_the_store_for_writing() {
    // The writable handle belongs to the recording path and nowhere
    // else. Two writers on one SQLite file is a lock fight at best.
    let writers: Vec<String> = game_sources()
        .into_iter()
        .filter(|(_, code)| code.contains("Store::open("))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        writers,
        vec!["telemetry.rs".to_owned()],
        "only the recording path may open the store for writing"
    );
}

#[test]
fn the_comment_stripper_does_not_eat_a_character_literal() {
    // ⚠️ The second way this tool can go blind, and the game crate
    // really does carry 22 character literals. `'a'` is a literal;
    // `&'a str` is a lifetime; and a stripper that guesses wrong
    // about the CLOSING quote starts a literal that never ends and
    // swallows code until the next apostrophe in the file — which
    // could be the very call this gate exists to find.
    let source = "let c = 'a'; Store::open(path)";
    assert!(code_only(source).contains("Store::open(path)"), "{source}");
    // The one that matters most: a literal that IS a slash.
    let slash = "let c = '/'; Store::open(path)";
    assert!(code_only(slash).contains("Store::open(path)"), "{slash}");
    // …and a lifetime must not open one either.
    let lifetime = "fn f<'a>(s: &'a str) {} Store::open(path)";
    assert!(
        code_only(lifetime).contains("Store::open(path)"),
        "{lifetime}"
    );
    // ⚠️ A lifetime whose quote is the ONLY one on the way to a
    // comment. A mutation probe found this case missing: with a
    // single quote and no partner, a reader that merely assumes a
    // literal has begun copies everything to the end of the file —
    // and every comment after it survives as code.
    let lonely = "fn f<'a>(x: u8) {}\n// mentions open_readonly\n";
    assert!(
        !code_only(lonely).contains("open_readonly"),
        "a lifetime opened a literal that never closed: {:?}",
        code_only(lonely)
    );
    // ⚠️ And the property that actually decides: a comment AFTER a
    // character literal must still be stripped. Getting the closing
    // quote wrong does not swallow code — every character is copied
    // either way — it leaves the stripper believing it is inside a
    // literal, so the comments that follow are read as code and the
    // gate raises a false alarm about its own prose.
    let after = "let c = 'a';\n// mentions open_readonly\nlet x = 1;";
    assert!(
        !code_only(after).contains("open_readonly"),
        "a comment after a character literal survived: {:?}",
        code_only(after)
    );
}

#[test]
fn the_comment_stripper_does_not_eat_a_url() {
    // ⚠️ The counter-check for this gate's own tool. A stripper that
    // treated the `//` in a URL as a comment would silently drop the
    // rest of that line, and this gate would pass because it could
    // no longer see what it forbids.
    let source = r#"
        let home = "https://example.com/a"; // a real comment
        /* a block
           comment */
        Store::open(path)
    "#;
    let code = code_only(source);
    assert!(code.contains("https://example.com/a"), "{code}");
    assert!(code.contains("Store::open(path)"), "{code}");
    assert!(!code.contains("a real comment"), "{code}");
    assert!(!code.contains("a block"), "{code}");
}
