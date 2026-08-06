//! `brain habits revive|fix <fuzzy>` — respawn a habit whose recurrence chain
//! has lapsed.
//!
//! A healthy recurring habit always has exactly one `not_started` occurrence
//! pending: `brain tasks complete` marks today's instance done and appends the
//! next one atomically. If an instance is ever marked done some other way (a
//! hand-edit, a bulk import) the "append next occurrence" step is skipped and
//! the chain silently dies — every remaining row is `done`, nothing is pending,
//! and the habit vanishes from the agenda.
//!
//! `revive` finds such a lapsed chain by fuzzy name and appends a fresh
//! occurrence, anchored to the last scheduled instance via the same
//! anchor-to-due recurrence math `complete` uses. A habit that still has a
//! pending occurrence needs no fix and is reported healthy.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::Result;
use chrono::{Local, NaiveDate};

use super::complete::{CsvFile, field, read_csv, spawn_next_occurrence, write_csv};
use crate::theme::Theme;

/// The result of a revive attempt against a single habit name (or a fuzzy
/// query that resolved to one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviveOutcome {
    /// No habit name matched the query.
    NoMatch,
    /// Several distinct habit names matched; the caller must disambiguate.
    Ambiguous(Vec<String>),
    /// The habit already has a pending (`not_started`) occurrence — nothing to do.
    Healthy(String),
    /// A fresh occurrence was appended for a lapsed chain.
    Revived {
        name: String,
        next_id: String,
        next_due: String,
    },
}

/// Deterministic fuzzy match: case-insensitive, requiring every
/// whitespace-separated token of `needle` to appear as a substring of `name`.
/// This tolerates word reordering ("send team status update" matches
/// "Send status update to team") without any nondeterministic scoring.
pub(crate) fn name_matches(name: &str, needle: &str) -> bool {
    let hay = name.to_ascii_lowercase();
    let tokens: Vec<&str> = needle.split_whitespace().collect();
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|token| hay.contains(token.to_ascii_lowercase().as_str()))
}

/// The distinct habit names matching `needle`, in stable first-appearance order.
pub(crate) fn matching_names(csv: &CsvFile, needle: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for row in &csv.rows {
        let name = field(row, "task_name");
        if name_matches(&name, needle) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Fuzzy entry point.
///
/// Resolves `needle` to a habit name and, when exactly one matches, revives it
/// (writing `habits.csv`). Zero matches → `NoMatch`; several → `Ambiguous` (no
/// write, so the caller can disambiguate).
#[cfg(test)]
fn revive_fuzzy_in_root(root: &Path, needle: &str, today: NaiveDate) -> Result<ReviveOutcome> {
    let habits_path = root.join("tasks").join("habits.csv");
    let csv = read_csv(&habits_path)?;
    let names = matching_names(&csv, needle);
    match names.as_slice() {
        [] => Ok(ReviveOutcome::NoMatch),
        [only] => revive_named_in_root(root, only, today),
        _ => Ok(ReviveOutcome::Ambiguous(names)),
    }
}

/// Revive an exact habit name (used after interactive disambiguation). Writes
/// `habits.csv` when it appends an occurrence.
pub(crate) fn revive_named_in_root(
    root: &Path,
    name: &str,
    today: NaiveDate,
) -> Result<ReviveOutcome> {
    let tasks_dir = root.join("tasks");
    let habits_path = tasks_dir.join("habits.csv");
    let mut csv = read_csv(&habits_path)?;
    match diagnose(&csv, name) {
        Health::Missing => Ok(ReviveOutcome::NoMatch),
        Health::Healthy => Ok(ReviveOutcome::Healthy(name.to_owned())),
        Health::Lapsed { source_idx } => {
            let (next_id, next_due) =
                spawn_next_occurrence(&tasks_dir, &mut csv, source_idx, today)?;
            write_csv(&habits_path, &csv)?;
            Ok(ReviveOutcome::Revived {
                name: name.to_owned(),
                next_id,
                next_due,
            })
        }
    }
}

/// The state of a habit chain for a given name.
enum Health {
    /// No rows carry this exact name.
    Missing,
    /// At least one occurrence is still pending (`status != done`) — no fix needed.
    Healthy,
    /// Every occurrence is `done`; `source_idx` is the latest-scheduled row to
    /// anchor the respawn to.
    Lapsed { source_idx: usize },
}

/// Classify the chain for `name`: healthy if any occurrence is pending,
/// otherwise lapsed, anchored to the row with the greatest `due_date`
/// (ties broken by the higher numeric id, i.e. the most recent instance).
fn diagnose(csv: &CsvFile, name: &str) -> Health {
    let mut any = false;
    let mut has_pending = false;
    let mut anchor: Option<(usize, NaiveDate, u32)> = None;
    for (idx, row) in csv.rows.iter().enumerate() {
        if field(row, "task_name") != name {
            continue;
        }
        any = true;
        if field(row, "status") != "done" {
            has_pending = true;
            continue;
        }
        let due = parse_due(&field(row, "due_date")).unwrap_or(NaiveDate::MIN);
        let id = id_number(&field(row, "task_id"));
        if anchor.is_none_or(|(_, best_due, best_id)| (due, id) > (best_due, best_id)) {
            anchor = Some((idx, due, id));
        }
    }
    if !any {
        Health::Missing
    } else if has_pending {
        Health::Healthy
    } else {
        // Every instance is done, so an anchor exists (`any && !has_pending`).
        Health::Lapsed {
            source_idx: anchor.map_or(0, |(idx, _, _)| idx),
        }
    }
}

fn parse_due(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

/// The numeric part of an `H###` id (0 if unparseable), for tie-breaking.
fn id_number(id: &str) -> u32 {
    id.trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .unwrap_or(0)
}

/// CLI runner for `brain habits revive|fix <query>`.
pub fn run(
    workspace: &crate::workspace::WorkspaceContext,
    query: &str,
    _actor: &crate::actor::ActorContext,
) -> Result<()> {
    let root = workspace.root();
    let today = Local::now().date_naive();
    let enabled = crate::config::Config::load(workspace).enable_triage_habits;
    let habits = read_csv(&root.join("tasks/habits.csv"))?;
    let names = matching_names(&habits, query);
    match names.as_slice() {
        [] => {
            let theme = Theme::active();
            eprintln!(
                "{}",
                theme.warning(&format!("No habit matches \"{query}\"."))
            );
        }
        [only] => {
            print_outcome(&revive_named_in_workspace(workspace, only, today, enabled)?);
        }
        names => {
            if let Some(chosen) = prompt_selection(names)? {
                let outcome = revive_named_in_workspace(workspace, &chosen, today, enabled)?;
                print_outcome(&outcome);
            }
        }
    }
    Ok(())
}

fn revive_named_in_workspace(
    workspace: &crate::workspace::WorkspaceContext,
    name: &str,
    today: NaiveDate,
    enabled: bool,
) -> Result<ReviveOutcome> {
    let _owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    protect_managed_revival(workspace.root(), name, enabled)?;
    revive_named_in_root(workspace.root(), name, today)
}

fn protect_managed_revival(root: &Path, name: &str, enabled: bool) -> Result<()> {
    let habits = read_csv(&root.join("tasks/habits.csv"))?;
    if let Some(row) = habits.rows.iter().find(|row| {
        field(row, "task_name") == name
            && crate::tasks::triage_habits::is_managed_system_key(&field(row, "system_key"))
    }) {
        crate::tasks::triage_habits::protect_system_key(
            &field(row, "system_key"),
            enabled,
            crate::tasks::triage_habits::ManagedTaskError::ManagedTaskCannotRevive,
        )?;
    }
    Ok(())
}

fn print_outcome(outcome: &ReviveOutcome) {
    let theme = Theme::active();
    match outcome {
        ReviveOutcome::Healthy(name) => eprintln!(
            "{} {}  {}",
            theme.success("healthy:"),
            theme.value(name),
            theme.muted("chain intact, no action needed")
        ),
        ReviveOutcome::Revived {
            name,
            next_id,
            next_due,
        } => {
            eprintln!("{} {}", theme.success("revived:"), theme.value(name));
            eprintln!(
                "  {} {} {} {}",
                theme.info("next occurrence:"),
                theme.accent(next_id),
                theme.muted("due"),
                theme.value(next_due)
            );
        }
        ReviveOutcome::NoMatch => eprintln!("{}", theme.warning("No matching habit.")),
        ReviveOutcome::Ambiguous(_) => {}
    }
}

/// Prompt the user to pick from the ambiguous candidates over `/dev/tty`.
/// Returns the chosen name, or `None` on cancel / no terminal / bad input.
fn prompt_selection(names: &[String]) -> Result<Option<String>> {
    let theme = Theme::active();
    let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
        eprintln!(
            "{}",
            theme.warning("Multiple habits match; re-run with a more specific query:")
        );
        for name in names {
            eprintln!("  - {}", theme.value(name));
        }
        return Ok(None);
    };
    let mut out = tty.try_clone()?;
    let mut reader = BufReader::new(tty);

    writeln!(out, "{}", theme.info("Multiple habits match. Which one?"))?;
    for (i, name) in names.iter().enumerate() {
        writeln!(out, "  {}. {}", i + 1, name)?;
    }
    write!(out, "Enter a number (blank to cancel): ")?;
    out.flush()?;

    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let choice = line.trim();
    if choice.is_empty() {
        return Ok(None);
    }
    match choice.parse::<usize>() {
        Ok(n) if (1..=names.len()).contains(&n) => Ok(Some(names[n - 1].clone())),
        _ => {
            writeln!(out, "{}", theme.warning("Not a valid choice; cancelled."))?;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
