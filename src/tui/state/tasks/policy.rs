use super::TasksState;

impl TasksState {
    pub(crate) fn validate_removal(
        &self,
        raw_id: &str,
        config: &crate::config::Config,
    ) -> Result<(), crate::tasks::triage_habits::ManagedTaskError> {
        self.all_tasks
            .iter()
            .chain(&self.all_habits)
            .find(|task| task.id.eq_ignore_ascii_case(raw_id))
            .map_or(Ok(()), |task| {
                crate::tasks::triage_habits::can_remove(task, config)
            })
    }
}
