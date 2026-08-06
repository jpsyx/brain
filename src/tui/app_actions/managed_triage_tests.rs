use super::{protect_completion, protect_removal};

fn managed() -> crate::tasks::task::Task {
    let mut task = crate::tasks::task::test_task("H7", "not_started");
    task.system_key = crate::tasks::triage_habits::DAILY_SYSTEM_KEY.to_owned();
    task
}

#[test]
fn actual_tui_mutation_guards_reject_managed_rows() {
    let task = managed();
    let config = crate::config::Config::default();

    assert!(protect_completion(&[], std::slice::from_ref(&task), "H7", &config).is_err());
    assert!(protect_removal(&[], &[task], "H7", &config).is_err());
}
