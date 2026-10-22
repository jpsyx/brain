//! The task aggregate owns base rows and rematerializes optimistic projections.
use super::TasksState;
use crate::{
    tasks::{
        identity::{CsvKind, legacy_task_uuid},
        removal::model::Identity,
    },
    tui::task_mutation::projection::Change,
    workspace::WorkspaceId,
};
impl TasksState {
    pub(crate) fn snapshot_revision(&self) -> u64 {
        self.snapshot_order.revision()
    }
    pub(crate) fn adopt_task_snapshot(
        &mut self,
        revision: u64,
        through: u64,
        rows: crate::tui::task_mutation::Rows,
    ) -> bool {
        if !self.snapshot_order.accepts(revision) {
            return false;
        }
        self.effects.observe_snapshot(through);
        self.adopt_rows(rows.0, rows.1);
        true
    }

    pub(crate) fn bind_workspace(&mut self, workspace: WorkspaceId) {
        self.workspace_id = Some(workspace);
    }
    pub(crate) fn completion_identity(
        &self,
        id: &str,
        workspace: WorkspaceId,
    ) -> Option<(Identity, bool)> {
        let task = self.row_with_id(id)?;
        let csv = if task.is_habit() {
            CsvKind::Habits
        } else {
            CsvKind::Tasks
        };
        Some((
            Identity {
                csv,
                task_uuid: task
                    .task_uuid
                    .unwrap_or_else(|| legacy_task_uuid(workspace, csv, &task.id)),
                task_id: task.id.clone(),
                task_name: task.name.clone(),
            },
            task.is_done(),
        ))
    }
    pub(crate) fn project_task_change(&mut self, job: u64, targets: Vec<Identity>, change: Change) {
        self.effects.insert(job, targets, change);
        self.project_changes();
    }
    pub(crate) fn mark_task_change_uncertain(&mut self, job: u64) {
        self.effects.uncertain(job);
        self.project_changes();
    }
    pub(crate) fn settle_task_change(&mut self, job: u64, committed: bool) {
        self.effects.settle(job, committed);
        self.project_changes();
    }
    pub(super) fn project_changes(&mut self) {
        let selected = self
            .selected_task()
            .map(|task| (task.task_uuid, task.id.clone()));
        if let Some(workspace) = self.workspace_id {
            for (kind, rows) in [
                (CsvKind::Tasks, &mut self.authoritative_tasks),
                (CsvKind::Habits, &mut self.authoritative_habits),
            ] {
                for row in rows {
                    row.task_uuid
                        .get_or_insert_with(|| legacy_task_uuid(workspace, kind, &row.id));
                }
            }
        }
        self.all_tasks = self
            .effects
            .project(CsvKind::Tasks, &self.authoritative_tasks);
        self.all_habits = self
            .effects
            .project(CsvKind::Habits, &self.authoritative_habits);
        self.rematerialize_active_view();
        if let Some((uuid, id)) = selected {
            if let Some(index) = self
                .visible_tasks
                .iter()
                .position(|row| uuid.map_or(row.id == id, |uuid| row.task_uuid == Some(uuid)))
            {
                self.selected_task = Some(index);
                self.ensure_selected_visible();
            }
        }
    }
}
