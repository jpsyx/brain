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
pub(crate) struct Effects {
    jobs: BTreeMap<u64, Effect>,
    observed_through: u64,
}
impl Effects {
    pub(crate) fn insert(&mut self, job: u64, targets: Vec<Identity>, change: Change) {
        self.jobs.insert(
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
        if let Some(effect) = self.jobs.get_mut(&job) {
            effect.uncertain = true;
        }
        self.retire_observed();
    }
    pub(crate) fn observe_refresh(&mut self) {
        self.jobs
            .retain(|_, effect| !effect.uncertain && !effect.committed);
    }
    pub(crate) fn observe_snapshot(&mut self, through: u64) {
        self.observed_through = self.observed_through.max(through);
        self.retire_observed();
    }
    fn retire_observed(&mut self) {
        self.jobs.retain(|job, effect| {
            *job > self.observed_through || (!effect.committed && !effect.uncertain)
        });
    }
    pub(crate) fn settle(&mut self, job: u64, committed: bool) {
        if committed {
            if let Some(effect) = self.jobs.get_mut(&job) {
                effect.committed = true;
            }
        } else {
            self.jobs.remove(&job);
        }
        self.retire_observed();
    }
    pub(crate) fn project(&self, kind: CsvKind, rows: &[Task]) -> Vec<Task> {
        let mut rows = rows.to_vec();
        for effect in self.jobs.values() {
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
    fn committed_effect_yields_to_post_commit_snapshot_even_after_reopen_or_restore() {
        let mut row = test_task("T1", "not_started");
        row.task_uuid = Some(TaskUuid::new());
        let target = Identity {
            csv: CsvKind::Tasks,
            task_uuid: row.task_uuid.unwrap(),
            task_id: row.id.clone(),
            task_name: row.name.clone(),
        };
        for change in [
            Change::Complete(chrono::Local::now().date_naive()),
            Change::Remove,
        ] {
            for snapshot_before_settlement in [false, true] {
                let mut effects = Effects::default();
                effects.insert(2, vec![target.clone()], change);
                effects.observe_snapshot(1);
                if snapshot_before_settlement {
                    effects.observe_snapshot(2);
                }
                effects.settle(2, true);
                if !snapshot_before_settlement {
                    let old = effects.project(CsvKind::Tasks, &[row.clone()]);
                    assert!(
                        old.is_empty() || old[0].is_done(),
                        "Older evidence keeps the commit projected"
                    );
                    effects.observe_snapshot(2);
                }
                let current = effects.project(CsvKind::Tasks, &[row.clone()]);
                assert_eq!(current.len(), 1);
                assert!(
                    !current[0].is_done(),
                    "Post-commit evidence owns the final visible state"
                );
                effects.insert(3, vec![target.clone()], change);
                effects.settle(3, true);
                effects.observe_refresh();
                assert!(!effects.project(CsvKind::Tasks, &[row.clone()])[0].is_done());
                effects.insert(4, vec![target.clone()], change);
                effects.observe_refresh();
                let pending = effects.project(CsvKind::Tasks, &[row.clone()]);
                assert!(
                    pending.is_empty() || pending[0].is_done(),
                    "Independent refresh preserves pending effects"
                );
            }
        }
    }

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
        effects.observe_refresh();
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
        effects.observe_snapshot(2);
        assert_eq!(effects.project(CsvKind::Tasks, &base[..1]).len(), 1);
    }
}
