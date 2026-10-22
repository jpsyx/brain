//! UUID-bound completion for the native task worker.
use super::{
    CompletionKind, CompletionResult, complete_habit, complete_task, field, read_csv, write_csv,
};
use crate::{
    tasks::{
        identity::{CsvKind, TaskUuid, legacy_task_uuid},
        removal::model::Identity,
        store_lock::Acquisition,
    },
    workspace::{CommandContext, WorkspaceId},
};
use anyhow::{Result, bail};
use chrono::NaiveDate;
use std::path::Path;

pub(crate) fn run(
    context: &CommandContext,
    identity: &Identity,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let owner = Acquisition::Queued.acquire(&context.workspace)?;
    owner.verify(&context.workspace)?;
    let result = complete_identity(
        context.workspace.root(),
        context.workspace.id(),
        identity,
        today,
    )?;
    drop(owner);
    Ok(result)
}

fn complete_identity(
    root: &Path,
    workspace: WorkspaceId,
    identity: &Identity,
    today: NaiveDate,
) -> Result<CompletionResult> {
    let dir = root.join("tasks");
    let path = dir.join(format!("{}.csv", identity.csv.as_str()));
    let mut csv = read_csv(&path)?;
    let mut matches = csv.rows.iter().enumerate().filter(|(_, row)| {
        let uuid = field(row, "task_uuid");
        let uuid = if uuid.trim().is_empty() {
            Some(legacy_task_uuid(
                workspace,
                identity.csv,
                &field(row, "task_id"),
            ))
        } else {
            TaskUuid::parse(&uuid).ok()
        };
        uuid == Some(identity.task_uuid)
    });
    let Some((index, row)) = matches.next() else {
        bail!("{} no longer exists", identity.task_id)
    };
    if matches.next().is_some() {
        bail!("{} has duplicate immutable identity", identity.task_id)
    }
    if field(row, "status") == "done" {
        return Ok(CompletionResult {
            kind: match identity.csv {
                CsvKind::Tasks => CompletionKind::Task,
                CsvKind::Habits => CompletionKind::Habit,
            },
            task_id: field(row, "task_id"),
            task_name: field(row, "task_name"),
            next_id: None,
            next_due: None,
            mit_migrated_to: None,
            project: None,
            linear_issue: None,
        });
    }
    let result = match identity.csv {
        CsvKind::Tasks => complete_task(&mut csv, index, today)?,
        CsvKind::Habits => complete_habit(&dir, &mut csv, index, today)?,
    };
    for column in ["status", "completed_date", "last_touched"] {
        super::ensure_column(&mut csv, column);
    }
    write_csv(&path, &csv)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{
        identity::{CsvKind, TaskUuid},
        removal::model::Identity,
    };
    #[test]
    fn queued_completion_resolves_uuid_and_is_idempotent_for_habits() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("tasks");
        std::fs::create_dir(&dir).unwrap();
        let uuid = TaskUuid::new();
        std::fs::write(dir.join("habits.csv"), format!("task_uuid,task_id,task_name,status,due_date,recur_interval,recur_unit,completed_date\n{uuid},H9,Example,not_started,2026-10-22,1,days,\n")).unwrap();
        let identity = Identity {
            csv: CsvKind::Habits,
            task_uuid: uuid,
            task_id: "H1".into(),
            task_name: "Example".into(),
        };
        let workspace_id = crate::workspace::WorkspaceId::new();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 22).unwrap();
        let result = complete_identity(root.path(), workspace_id, &identity, date).unwrap();
        assert_eq!(result.task_id, "H9");
        let before = std::fs::read(dir.join("habits.csv")).unwrap();
        complete_identity(root.path(), workspace_id, &identity, date).unwrap();
        assert_eq!(std::fs::read(dir.join("habits.csv")).unwrap(), before);
        let missing = Identity {
            task_uuid: TaskUuid::new(),
            ..identity
        };
        assert!(complete_identity(root.path(), workspace_id, &missing, date).is_err());
    }
}
