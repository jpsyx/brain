//! `brain reindex`: rebuild the derived lookup CSVs (and re-apply the task /
//! habit automation rules) from the canonical `.METADATA.json` + `notes.md`
//! sources.
//!
//! The lookup CSVs (`projects/projects-lookup.csv`, `resources/zotero-lookup.csv`)
//! are *derived indexes*: this command regenerates them so they mirror the
//! canonical sources after edits. A bare `brain reindex` rebuilds all three
//! families; `--projects` / `--resources` / `--tasks` narrow the run.
//!
//! Decision logic is pure and tested in the sibling modules; this module is the
//! thin themed IO shell.

mod csvfmt;
pub mod notes;
pub mod projects;
pub mod resources;
mod select;
mod tasks;
mod walk;

use select::selection;

use std::path::PathBuf;

use anyhow::{Result, anyhow};

use crate::theme::Theme;

/// Run `brain reindex`. Resolves the brain root, then rebuilds the selected
/// lookup families, narrating each phase.
pub fn run(projects: bool, resources: bool, tasks_flag: bool) -> Result<()> {
    let root = crate::paths::brain_root()?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("$HOME is not set"))?;
    let sel = selection(projects, resources, tasks_flag);
    let theme = Theme::active();

    println!("{}", theme.heading("Reindexing brain derived data"));
    println!("{}", theme.muted(&format!("root: {}", root.display())));

    if sel.projects {
        println!(
            "{}",
            theme.muted("Rebuilding projects-lookup.csv from projects/…")
        );
        let report = walk::rebuild_projects(&root)?;
        println!(
            "{}",
            theme.success(&format!("✓ {}", summarize("projects-lookup.csv", &report)))
        );
    }
    if sel.resources {
        println!(
            "{}",
            theme.muted("Rebuilding zotero-lookup.csv from resources/…")
        );
        let report = walk::rebuild_resources(&root)?;
        println!(
            "{}",
            theme.success(&format!("✓ {}", summarize("zotero-lookup.csv", &report)))
        );
    }
    if sel.tasks {
        println!(
            "{}",
            theme.muted("Applying task + habit automation rules…")
        );
        let outcome = tasks::reindex_tasks(&root, &home);
        println!("{}", tasks::format_task_outcome(&outcome, theme));
    }
    Ok(())
}

/// One-line summary of a rebuilt lookup family. Pure.
#[must_use]
fn summarize(name: &str, report: &walk::Report) -> String {
    if !report.wrote {
        return format!("{name}: nothing to index");
    }
    let skipped = if report.skipped > 0 {
        format!(" ({} skipped: invalid .METADATA.json)", report.skipped)
    } else {
        String::new()
    };
    format!("{name}: {} rows{skipped}", report.rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_reports_row_count() {
        let report = walk::Report {
            path: PathBuf::from("projects/projects-lookup.csv"),
            rows: 34,
            skipped: 0,
            wrote: true,
        };
        assert_eq!(summarize("projects-lookup.csv", &report), "projects-lookup.csv: 34 rows");
    }

    #[test]
    fn summarize_notes_skipped_invalid_metadata() {
        let report = walk::Report {
            path: PathBuf::from("x"),
            rows: 2,
            skipped: 1,
            wrote: true,
        };
        assert!(summarize("zotero-lookup.csv", &report).contains("1 skipped"));
    }

    #[test]
    fn summarize_reports_nothing_to_index_when_dir_absent() {
        let report = walk::Report {
            path: PathBuf::from("x"),
            rows: 0,
            skipped: 0,
            wrote: false,
        };
        assert_eq!(
            summarize("projects-lookup.csv", &report),
            "projects-lookup.csv: nothing to index"
        );
    }
}
