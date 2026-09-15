//! The roster and the statistics, on the command line.
//!
//! The reading side of [`beatbyte_core::stats`] — and deliberately
//! the SAME functions the game's screens draw from. Two
//! implementations of "accuracy over time" would eventually disagree
//! in front of the player, so this prints what the game plots and is
//! the way to check one against the other.

use std::path::PathBuf;
use std::process::ExitCode;

use beatbyte_core::history::{PlayEntry, claim_unattributed, iso_utc, parse_log};
use beatbyte_core::player::{PlayerId, Roster};
use beatbyte_core::stats::{self, Filter};

/// Where the game keeps the roster.
#[must_use]
pub fn roster_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("players.json"))
}

/// Where the game keeps the play log.
#[must_use]
pub fn history_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("history.jsonl"))
}

/// Load the roster, or an empty one.
#[must_use]
pub fn load_roster() -> Roster {
    roster_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Load the play log, or an empty one.
#[must_use]
pub fn load_history() -> Vec<PlayEntry> {
    history_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map_or_else(Vec::new, |text| parse_log(&text))
}

/// Write the roster back.
///
/// # Errors
/// When the file cannot be written.
pub fn save_roster(roster: &Roster) -> Result<(), String> {
    let path = roster_path().ok_or("no data directory on this platform")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(roster).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| error.to_string())
}

/// List the roster, or add to it.
///
/// The exit code is the house convention: 2 for a refusal the user
/// can fix (a blank or taken name), 0 otherwise.
#[must_use]
pub fn run(add_name: Option<&str>) -> ExitCode {
    match add_name {
        Some(name) => match add(name) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cannot add `{name}`: {error}");
                ExitCode::from(2)
            }
        },
        None => {
            list();
            ExitCode::SUCCESS
        }
    }
}

/// Print one player's statistics.
#[must_use]
pub fn stats(name: Option<&str>) -> ExitCode {
    match report(name) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

/// Print the roster.
pub fn list() {
    let roster = load_roster();
    let history = load_history();
    if roster.players.is_empty() {
        println!("no players yet — `beatbyte-cli players add <name>`");
    }
    for player in &roster.players {
        let runs = stats::runs_of(&history, player.id, Filter::default());
        let summary = stats::summarize(&runs);
        let playing = if roster.selected == Some(player.id) {
            " (playing)"
        } else {
            ""
        };
        println!(
            "{:>4}  {:<24} {:>4} runs  {:>3} songs{playing}",
            player.id, player.name, summary.runs, summary.songs
        );
    }
    let loose = stats::unattributed(&history, Filter::default());
    if !loose.is_empty() {
        println!("\n{} runs are credited to nobody", loose.len());
    }
}

/// Add a player, adopting the runs that predate the roster when this
/// is the first one.
///
/// # Errors
/// When the name is refused or the roster cannot be written.
pub fn add(name: &str) -> Result<(), String> {
    let mut roster = load_roster();
    let adopting = roster.adopts_orphans();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    let id = roster
        .add(name, now)
        .map_err(|error| error.message().to_lowercase())?;
    if adopting {
        let claimed = adopt(id)?;
        roster.mark_adopted();
        if claimed > 0 {
            println!("{claimed} earlier runs are now {name}'s");
        }
    }
    save_roster(&roster)?;
    println!("added {name} (id {id})");
    Ok(())
}

/// Credit the unattributed runs to a player, keeping a copy of the
/// log first.
fn adopt(player: PlayerId) -> Result<usize, String> {
    let Some(path) = history_path() else {
        return Ok(0);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(0); // no log yet is not a failure
    };
    let backup = path.with_extension(format!(
        "pre-roster.{}.jsonl",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs())
    ));
    // The copy goes down BEFORE the rewrite and is never removed:
    // this is the one command that edits somebody's play history.
    std::fs::write(&backup, &text).map_err(|error| error.to_string())?;
    let (rewritten, claimed) = claim_unattributed(&text, player);
    std::fs::write(&path, rewritten).map_err(|error| error.to_string())?;
    println!("a copy of the log is at {}", backup.display());
    Ok(claimed)
}

/// Print one player's statistics — the same numbers the game plots.
///
/// # Errors
/// When no such player exists.
pub fn report(name: Option<&str>) -> Result<(), String> {
    let roster = load_roster();
    let history = load_history();
    let player = match name {
        Some(wanted) => roster
            .players
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| format!("no player called `{wanted}`"))?,
        None => roster
            .current()
            .ok_or("nobody is selected; name a player or pick one in the game")?,
    };
    let runs = stats::runs_of(&history, player.id, Filter::default());
    let summary = stats::summarize(&runs);

    println!("{}", player.name);
    println!(
        "  {} runs, {} finished, {} songs, {:.0} minutes played",
        summary.runs,
        summary.completed,
        summary.songs,
        summary.seconds / 60.0
    );
    match (
        summary.best_accuracy,
        summary.mean_accuracy,
        summary.best_score,
    ) {
        (Some(best), Some(mean), Some(score)) => println!(
            "  best {:.1} % accuracy, best score {score}, mean {:.1} % (finished runs)",
            best * 100.0,
            mean * 100.0
        ),
        // Nothing finished: "0 %" would read as "played and failed".
        _ => println!("  no finished run yet, so no accuracy to report"),
    }
    match summary.mean_offset_ms {
        Some(drift) => println!("  timing: {drift:+.1} ms on average (negative is early)"),
        None => println!("  timing: not recorded on any counted run"),
    }
    if let Some(last) = summary.last_played_ms {
        println!("  last played {}", iso_utc(last));
    }

    println!("\n  by difficulty");
    for stat in stats::by_difficulty(&runs) {
        if stat.runs == 0 {
            println!("    {:<8} never played", stat.difficulty.display_name());
            continue;
        }
        match (stat.best_accuracy, stat.mean_accuracy) {
            (Some(best), Some(mean)) => println!(
                "    {:<8} {:>3} runs, {:>3} finished, best {:.1} %, mean {:.1} %",
                stat.difficulty.display_name(),
                stat.runs,
                stat.completed,
                best * 100.0,
                mean * 100.0
            ),
            _ => println!(
                "    {:<8} {:>3} runs, none finished",
                stat.difficulty.display_name(),
                stat.runs
            ),
        }
    }

    let points = stats::progression(&runs);
    let accuracies: Vec<f64> = points.iter().map(|p| p.accuracy).collect();
    match stats::trend(&accuracies) {
        Some(slope) => println!(
            "\n  trend over {} finished runs: {:+.2} points per 10 runs",
            points.len(),
            slope * 10.0 * 100.0
        ),
        None => println!("\n  not enough finished runs for a trend"),
    }

    for other in roster.players.iter().filter(|p| p.id != player.id) {
        let duels = stats::head_to_head(&history, player.id, other.id);
        if duels.is_empty() {
            println!(
                "\n  vs {}: no song both have finished at the same difficulty",
                other.name
            );
            continue;
        }
        let ahead = duels.iter().filter(|duel| duel.margin() > 0.0).count();
        println!(
            "\n  vs {}: ahead on {ahead} of {} shared songs",
            other.name,
            duels.len()
        );
        for duel in duels.iter().take(10) {
            println!(
                "    {:<34} {:<7} {:>5.1} % vs {:>5.1} %",
                truncate(&duel.title, 34),
                duel.difficulty,
                duel.theirs * 100.0,
                duel.others * 100.0
            );
        }
    }
    Ok(())
}

/// Shorten a title to fit a column, on character boundaries.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

/// Print one player's achievements — the same evaluation the game runs.
///
/// Exists for the same reason `stats` does: the game's screen and this
/// share one implementation, so a number that is wrong is wrong in
/// both and can be cross-checked against the raw log by hand. It is
/// also the only way to read the list on a machine whose screen
/// cannot be photographed.
pub fn awards(name: Option<&str>, locked: bool) -> ExitCode {
    match award_report(name, locked) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The body of [`awards`].
///
/// # Errors
/// When no such player exists.
pub fn award_report(name: Option<&str>, locked: bool) -> Result<(), String> {
    use beatbyte_core::achievements::{CATALOGUE, Category, earned_at, evaluate};

    let roster = load_roster();
    let history = load_history();
    let player = match name {
        Some(wanted) => roster
            .players
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| format!("no player called `{wanted}`"))?,
        None => roster
            .current()
            .ok_or("nobody is selected; name a player or pick one in the game")?,
    };
    // Straight from the log, exactly as the game sweeps it — this
    // deliberately does NOT read `achievements.json`, so the two can
    // be compared rather than agreeing by construction.
    let when: std::collections::BTreeMap<String, u64> =
        earned_at(&history, player.id).into_iter().collect();
    let progress = evaluate(&history, player.id);
    let hidden_found = CATALOGUE
        .iter()
        .filter(|entry| entry.hidden && when.contains_key(entry.id))
        .count();

    println!("{}", player.name);
    println!(
        "  {} of {} earned, {} of them secret",
        when.len(),
        CATALOGUE.len(),
        hidden_found
    );
    for category in Category::ALL {
        let rows: Vec<(usize, &beatbyte_core::achievements::Achievement)> = CATALOGUE
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.category == category)
            .filter(|(_, entry)| locked || when.contains_key(entry.id))
            .collect();
        if rows.is_empty() {
            continue;
        }
        println!("\n{}", category.label());
        for (at, entry) in rows {
            match when.get(entry.id) {
                Some(ms) => {
                    let day = beatbyte_core::history::iso_utc(*ms);
                    println!("  [x] {:<22} {}", entry.title, &day[..10.min(day.len())]);
                }
                None if entry.hidden => println!("  [ ] {:<22} (secret)", "? ? ?"),
                None => {
                    let step = progress[at];
                    if step.need > 1.0 {
                        println!(
                            "  [ ] {:<22} {:.0} / {:.0}",
                            entry.title, step.have, step.need
                        );
                    } else {
                        println!("  [ ] {:<22} {}", entry.title, entry.tier.label());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_shortened_on_character_boundaries() {
        // Titles carry umlauts and accents; a byte cut would panic.
        assert_eq!(truncate("Africa", 10), "Africa");
        assert_eq!(truncate("ääääääää", 4).chars().count(), 4);
        assert!(truncate("a very long song title indeed", 10).ends_with('…'));
    }
}
