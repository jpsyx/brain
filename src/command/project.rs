//! `brain project` — dispatch, output, and the reindex every mutation owes.
//!
//! A `.METADATA.json` the lookup CSV does not know about is the most common way
//! this tree drifts from itself, so each mutation rebuilds the project lookup
//! before it reports.

use anyhow::Result;
use chrono::Local;

use crate::cli::{ProjectAction, ProjectArgs};
use crate::project::{self, Change, Report};
use crate::theme::Theme;
use crate::workspace::CommandContext;

pub fn run(args: &ProjectArgs, context: &CommandContext) -> Result<()> {
    let root = context.workspace.root();
    let theme = Theme::active();
    match &args.action {
        ProjectAction::New(new) => {
            let created = project::create(
                root,
                &new.slug,
                &new.title,
                &new.status,
                &new.priority,
                &new.due,
                &new.description,
            )?;
            reindex_projects(context)?;
            eprint!(
                "{}",
                render_created(
                    &created.slug,
                    &created.directory.display().to_string(),
                    theme
                )
            );
            Ok(())
        }
        ProjectAction::Set(set) => {
            let (located, changes) = project::set(
                root,
                &set.slug,
                set.title.as_deref(),
                set.status.as_deref(),
                set.priority.as_deref(),
                set.due.as_deref(),
            )?;
            if !changes.is_empty() {
                reindex_projects(context)?;
            }
            eprint!("{}", render_changes(&located.slug, &changes, theme));
            Ok(())
        }
        ProjectAction::Archive(slug) => {
            let located = project::archive(root, &slug.slug)?;
            reindex_projects(context)?;
            eprintln!(
                "{} {}  {}",
                theme.success("archived:"),
                theme.accent(&located.slug),
                theme.muted(&located.relative)
            );
            Ok(())
        }
        ProjectAction::Show(show) => {
            let report = project::show(root, &show.slug, Local::now().date_naive())?;
            if show.json {
                println!("{}", serde_json::to_string(&report)?);
            } else {
                eprint!("{}", render_report(&report, theme));
            }
            Ok(())
        }
    }
}

/// Rebuild `projects-lookup.csv` so it mirrors what just changed on disk.
fn reindex_projects(context: &CommandContext) -> Result<()> {
    crate::reindex::run(&context.workspace, true, false, false)
}

fn render_created(slug: &str, directory: &str, theme: Theme) -> String {
    format!(
        "{} {}\n  {}\n",
        theme.success("created:"),
        theme.accent(slug),
        theme.muted(directory)
    )
}

fn render_changes(slug: &str, changes: &[Change], theme: Theme) -> String {
    use std::fmt::Write as _;

    if changes.is_empty() {
        return format!(
            "{} {}  {}\n",
            theme.info("unchanged:"),
            theme.accent(slug),
            theme.muted("(already had those values)")
        );
    }
    let mut out = format!("{} {}\n", theme.success("updated:"), theme.accent(slug));
    for change in changes {
        let _ = writeln!(
            out,
            "  {} {} → {}",
            theme.muted(&format!("{}:", change.field)),
            theme.muted(&change.before),
            theme.value(&change.after)
        );
    }
    out
}

fn render_report(report: &Report, theme: Theme) -> String {
    use std::fmt::Write as _;

    let mut out = format!(
        "{} {}\n  {} {}\n  {} {}  {}  {}\n",
        theme.heading(&report.title),
        theme.muted(&format!("({})", report.slug)),
        theme.muted("directory:"),
        theme.value(&report.directory),
        theme.muted("status:"),
        theme.value(&report.status),
        theme.value(&report.priority),
        theme.muted(&format!("due {}", report.due))
    );
    let _ = writeln!(
        out,
        "  {} {} open, {} ignored",
        theme.muted("tasks:"),
        theme.value(&report.open_tasks.len().to_string()),
        theme.value(&report.ignored_tasks.len().to_string())
    );
    if report.died_quietly {
        let _ = writeln!(
            out,
            "  {}",
            theme.warning(&format!(
                "all {} open task(s) have been ignored for weeks — this project \
                 probably stopped rather than finished",
                report.open_tasks.len()
            ))
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{render_changes, render_created, render_report};
    use crate::project::{Change, Report};
    use crate::theme::Theme;

    fn plain() -> Theme {
        Theme::dark(false)
    }

    fn report(open: usize, ignored: usize) -> Report {
        Report {
            slug: "work__apply".to_owned(),
            directory: "projects/work__apply".to_owned(),
            archived: false,
            title: "Apply".to_owned(),
            status: "in-progress".to_owned(),
            priority: "p1".to_owned(),
            due: "none".to_owned(),
            open_tasks: (0..open).map(|n| format!("T{n}")).collect(),
            ignored_tasks: (0..ignored).map(|n| format!("T{n}")).collect(),
            died_quietly: open > 0 && open == ignored,
        }
    }

    #[test]
    fn a_new_project_reports_where_it_landed() {
        let out = render_created("work__apply", "/b/projects/work__apply", plain());
        assert!(out.contains("created: work__apply"), "{out}");
        assert!(out.contains("/b/projects/work__apply"), "{out}");
    }

    #[test]
    fn a_no_op_set_says_so_rather_than_claiming_an_update() {
        let out = render_changes("work__apply", &[], plain());
        assert!(out.contains("unchanged: work__apply"), "{out}");
    }

    #[test]
    fn a_set_lists_each_field_that_moved() {
        let changes = [Change {
            field: "status",
            before: "in-progress".to_owned(),
            after: "done".to_owned(),
        }];
        let out = render_changes("work__apply", &changes, plain());
        assert!(out.contains("status: in-progress → done"), "{out}");
    }

    #[test]
    fn a_project_that_died_quietly_is_called_out() {
        let out = render_report(&report(3, 3), plain());
        assert!(out.contains("3 open, 3 ignored"), "{out}");
        assert!(
            out.contains("probably stopped rather than finished"),
            "{out}"
        );
    }

    #[test]
    fn a_live_project_gets_no_warning() {
        let out = render_report(&report(3, 1), plain());
        assert!(!out.contains("probably stopped"), "{out}");
    }

    #[test]
    fn a_finished_project_is_not_reported_as_abandoned() {
        let out = render_report(&report(0, 0), plain());
        assert!(!out.contains("probably stopped"), "{out}");
    }
}
