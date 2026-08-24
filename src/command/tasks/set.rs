use anyhow::Result;

/// The editable fields offered by the interactive picker, in prompt order.
/// Each entry is (menu label, the `Edit` slot it fills).
const SET_FIELDS: [(&str, &str); 6] = [
    ("due date", "due"),
    ("priority", "priority"),
    ("title", "name"),
    ("status", "status"),
    ("notes", "notes"),
    ("project", "project"),
];

pub(super) fn run(
    store: &crate::workspace::RegistryStore,
    workspace: &crate::workspace::WorkspaceContext,
    args: crate::tasks::cli::SetArgs,
) -> Result<()> {
    let mut edit = crate::tasks::set::Edit {
        name: args.name,
        due: args.due,
        priority: args.priority,
        status: args.status,
        notes: args.notes,
        project: args.project,
        linear_issue: args.linear_issue,
        duration: args.duration,
        ideal_time: args.ideal_time,
        habit: args.habit,
    };
    if edit.is_empty() {
        prompt_for_field(&mut edit, &args.id)?;
    }
    let plan = crate::tasks::set::set_in_workspace(store, workspace, &args.id, &edit)?;
    if args.json {
        println!("{}", serde_json::to_string(&SetReport::from(&plan))?);
    } else {
        eprint!("{}", format_set_plan(&plan, crate::theme::Theme::active()));
    }
    Ok(())
}

/// Ask a human which field to change and what to change it to.
///
/// Only reached when no field flag was passed at all, so an agent never lands
/// here. With no TTY there is nothing to ask, and the empty edit falls through
/// to `plan`'s "pass at least one of …" error.
fn prompt_for_field(edit: &mut crate::tasks::set::Edit, id: &str) -> Result<()> {
    use std::fmt::Write as _;

    let theme = crate::theme::Theme::active();
    let mut menu = format!("{}\n", theme.heading(&format!("Edit {id}")));
    for (index, (label, _)) in SET_FIELDS.iter().enumerate() {
        let _ = writeln!(
            menu,
            "  {} {}",
            theme.accent(&format!("{}.", index + 1)),
            theme.value(label)
        );
    }
    eprint!("{menu}");
    let Some(answer) = crate::command::configuration::prompt_tty_line(&theme.prompt("field: "))?
    else {
        return Ok(());
    };
    let answer = answer.trim().to_ascii_lowercase();
    let Some((label, slot)) = SET_FIELDS
        .iter()
        .enumerate()
        .find(|(index, (label, slot))| {
            answer == (index + 1).to_string() || answer == *label || answer == *slot
        })
        .map(|(_, entry)| *entry)
    else {
        anyhow::bail!("'{answer}' is not one of the editable fields");
    };
    let Some(value) =
        crate::command::configuration::prompt_tty_line(&theme.prompt(&format!("new {label}: ")))?
    else {
        return Ok(());
    };
    let value = value.trim_end_matches(['\n', '\r']).to_owned();
    match slot {
        "due" => edit.due = Some(value),
        "priority" => edit.priority = Some(value),
        "name" => edit.name = Some(value),
        "status" => edit.status = Some(value),
        "notes" => edit.notes = Some(value),
        _ => edit.project = Some(value),
    }
    Ok(())
}

/// JSON shape for `--json`: the row touched plus one entry per changed column.
#[derive(serde::Serialize)]
struct SetReport<'a> {
    task_id: &'a str,
    task_name: &'a str,
    kind: &'static str,
    changed: Vec<SetReportChange<'a>>,
}

#[derive(serde::Serialize)]
struct SetReportChange<'a> {
    field: &'a str,
    from: &'a str,
    to: &'a str,
}

impl<'a> From<&'a crate::tasks::set::SetPlan> for SetReport<'a> {
    fn from(plan: &'a crate::tasks::set::SetPlan) -> Self {
        Self {
            task_id: &plan.task_id,
            task_name: &plan.task_name,
            kind: if plan.is_habit { "habit" } else { "task" },
            changed: plan
                .changes
                .iter()
                .map(|change| SetReportChange {
                    field: &change.column,
                    from: &change.before,
                    to: &change.after,
                })
                .collect(),
        }
    }
}

fn format_set_plan(plan: &crate::tasks::set::SetPlan, theme: crate::theme::Theme) -> String {
    use std::fmt::Write as _;

    if plan.is_noop() {
        return format!(
            "{} {}  {} {}\n",
            theme.info("unchanged:"),
            theme.accent(&plan.task_id),
            theme.value(&plan.task_name),
            theme.muted("(already had those values)")
        );
    }
    let mut out = format!(
        "{} {}  {}{}\n",
        theme.success("updated:"),
        theme.accent(&plan.task_id),
        theme.value(&plan.task_name),
        if plan.is_habit {
            format!(" {}", theme.muted("(habit)"))
        } else {
            String::new()
        }
    );
    for change in &plan.changes {
        let before = if change.before.trim().is_empty() {
            "(empty)".to_owned()
        } else {
            change.before.clone()
        };
        let after = if change.after.trim().is_empty() {
            "(empty)".to_owned()
        } else {
            change.after.clone()
        };
        let _ = writeln!(
            out,
            "  {} {} {} {}",
            theme.muted(&format!("{}:", change.column)),
            theme.muted(&before),
            theme.muted("→"),
            theme.value(&after)
        );
    }
    out
}
