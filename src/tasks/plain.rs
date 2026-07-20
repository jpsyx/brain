//! `--no-tui` fallback: prints the same task data without curses / colors.

use chrono::NaiveDate;

use crate::tasks::render::{status_label, truncate, type_label};
use crate::tasks::task::Task;
use crate::tasks::view::ViewSpec;

pub fn print_plain(view: &ViewSpec, today: NaiveDate, full_notes: bool) {
    println!("== {} ==", view.title);
    if !view.subtitle.is_empty() {
        println!("   {}", view.subtitle);
    }
    println!("   {} shown / {} total", view.tasks.len(), view.total);
    println!();
    if view.tasks.is_empty() {
        println!("(no tasks match)");
        return;
    }
    for t in &view.tasks {
        print_task(t, today, full_notes);
    }
}

fn print_task(t: &Task, today: NaiveDate, full_notes: bool) {
    let due = format_due(t.due_date, today);
    let hard = if t.hard_deadline { " ⚠HARD" } else { "" };
    let types = t
        .types
        .iter()
        .map(|x| type_label(x))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "[{}] {} {} {}{}  — {}",
        t.id,
        t.priority.to_ascii_uppercase(),
        types,
        t.name,
        hard,
        due,
    );

    let mut meta: Vec<String> = vec![status_label(&t.status).to_owned()];
    if !t.project.is_empty() {
        meta.push(format!("project={}", t.project));
    }
    if !t.context.is_empty() {
        meta.push(format!("@{}", t.context));
    }
    if !t.energy.is_empty() {
        meta.push(format!("{} energy", t.energy));
    }
    if let Some(m) = t.estimated_duration {
        meta.push(format!("{m}m"));
    }
    if !t.blocked_by.is_empty() {
        meta.push(format!("blocked-by {}", t.blocked_by.join(",")));
    }
    println!("        {}", meta.join("  ·  "));

    if !t.notes.trim().is_empty() {
        let body = if full_notes {
            t.notes.replace('\n', " ↵ ")
        } else {
            truncate(&t.notes, 120)
        };
        println!("        📝 {body}");
    }
    if !t.see_also.trim().is_empty() {
        println!("        🔗 {}", truncate(&t.see_also, 120));
    }
    println!();
}

fn format_due(due: Option<NaiveDate>, today: NaiveDate) -> String {
    due.map_or_else(
        || "no due date".to_owned(),
        |d| {
            let diff = (d - today).num_days();
            if diff < 0 {
                format!("OVERDUE {}d ({d})", -diff)
            } else if diff == 0 {
                format!("TODAY ({d})")
            } else if diff == 1 {
                format!("tomorrow ({d})")
            } else {
                format!("in {diff}d ({d})")
            }
        },
    )
}
