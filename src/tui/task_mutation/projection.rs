//! Job-scoped overlays over the latest authoritative rows.
use crate::tasks::{identity::CsvKind, removal::model::Identity, task::Task};
use chrono::NaiveDate;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) enum Change {
    Complete(NaiveDate),
    Remove,
}
struct Effect {
    targets: Vec<Identity>,
    change: Change,
    committed: bool,
    uncertain: bool,
}
#[derive(Default)]
pub(crate) struct Effects(BTreeMap<u64, Effect>);
impl Effects {
    pub(crate) fn insert(&mut self, job: u64, targets: Vec<Identity>, change: Change) {
        self.0.insert(
            job,
            Effect {
                targets,
                change,
                committed: false,
                uncertain: false,
            },
        );
    }
    pub(crate) fn uncertain(&mut self, job: u64) {
        if let Some(effect) = self.0.get_mut(&job) {
            effect.uncertain = true;
        }
    }
    pub(crate) fn reconcile_uncertain(&mut self) {
        self.0.retain(|_, effect| !effect.uncertain);
    }
    pub(crate) fn settle(&mut self, job: u64, committed: bool) {
        if committed {
            if let Some(effect) = self.0.get_mut(&job) {
                effect.committed = true;
            }
        } else {
            self.0.remove(&job);
        }
    }
    pub(crate) fn reconcile(&mut self, kind: CsvKind, rows: &[Task]) {
        self.0.retain(|_, effect| {
            if !effect.committed {
                return true;
            }
            effect.targets.retain(|target| {
                if target.csv != kind {
                    return true;
                }
                let row = rows
                    .iter()
                    .find(|row| row.task_uuid == Some(target.task_uuid));
                match effect.change {
                    Change::Remove => row.is_some(),
                    Change::Complete(_) => row.is_some_and(|row| !row.is_done()),
                }
            });
            !effect.targets.is_empty()
        });
    }
    pub(crate) fn project(&self, kind: CsvKind, rows: &[Task]) -> Vec<Task> {
        let mut rows = rows.to_vec();
        for effect in self.0.values() {
            for target in &effect.targets {
                if target.csv != kind {
                    continue;
                }
                match effect.change {
                    Change::Remove => rows.retain(|row| row.task_uuid != Some(target.task_uuid)),
                    Change::Complete(date) => {
                        if let Some(row) = rows
                            .iter_mut()
                            .find(|row| row.task_uuid == Some(target.task_uuid))
                        {
                            row.status = "done".into();
                            row.completed_date = Some(date);
                            row.last_touched = Some(date);
                        }
                    }
                }
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{
        identity::{CsvKind, TaskUuid},
        removal::model::Identity,
        task::test_task,
    };
    #[test]
    fn uncertain_effect_waits_for_new_authoritative_evidence() {
        let mut row = test_task("T1", "not_started");
        row.task_uuid = Some(TaskUuid::new());
        let identity = Identity {
            csv: CsvKind::Tasks,
            task_uuid: row.task_uuid.unwrap(),
            task_id: row.id.clone(),
            task_name: row.name.clone(),
        };
        let mut effects = Effects::default();
        effects.insert(1, vec![identity], Change::Remove);
        effects.uncertain(1);
        assert!(effects.project(CsvKind::Tasks, &[row.clone()]).is_empty());
        effects.reconcile_uncertain();
        assert_eq!(effects.project(CsvKind::Tasks, &[row]).len(), 1);
    }
    #[test]
    fn settlement_preserves_other_effects_and_authoritative_edits() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 22).unwrap();
        let mut a = test_task("T1", "not_started");
        a.task_uuid = Some(TaskUuid::new());
        let mut b = test_task("T2", "not_started");
        b.task_uuid = Some(TaskUuid::new());
        let identity = |task: &crate::tasks::task::Task| Identity {
            csv: CsvKind::Tasks,
            task_uuid: task.task_uuid.unwrap(),
            task_id: task.id.clone(),
            task_name: task.name.clone(),
        };
        let mut effects = Effects::default();
        effects.insert(1, vec![identity(&a)], Change::Complete(date));
        effects.insert(2, vec![identity(&b)], Change::Remove);
        a.name = "Edited by sync".into();
        let base = vec![a, b];
        assert_eq!(effects.project(CsvKind::Tasks, &base).len(), 1);
        effects.settle(1, false);
        let rows = effects.project(CsvKind::Tasks, &base);
        assert_eq!(rows[0].status, "not_started");
        assert_eq!(rows[0].name, "Edited by sync");
        effects.settle(2, true);
        assert_eq!(
            effects.project(CsvKind::Tasks, &base).len(),
            1,
            "committed removal survives stale refresh"
        );
        effects.reconcile(CsvKind::Tasks, &base[..1]);
        assert_eq!(effects.project(CsvKind::Tasks, &base[..1]).len(), 1);
    }
}
