use super::protect_removal;

fn managed() -> crate::tasks::task::Task {
    let mut task = crate::tasks::task::test_task("H7", "not_started");
    task.system_key = crate::tasks::triage_habits::DAILY_SYSTEM_KEY.to_owned();
    task
}

/// The TUI guards deletion of a managed row, and only deletion: marking one
/// complete is the user doing their triage by hand.
#[test]
fn actual_tui_mutation_guards_reject_only_managed_removal() {
    let task = managed();
    let config = crate::config::Config::default();

    assert!(protect_removal(&[], std::slice::from_ref(&task), "H7", &config).is_err());
}
