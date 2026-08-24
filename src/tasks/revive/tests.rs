use super::*;

#[test]
fn command_runner_requires_explicit_workspace_and_actor_contexts() {
    fn accepts_runner(
        _: fn(
            &crate::workspace::RegistryStore,
            &crate::workspace::WorkspaceContext,
            &str,
            &crate::actor::ActorContext,
        ) -> anyhow::Result<()>,
    ) {
    }
    accepts_runner(super::run);
}

const HEADER: &str = "task_id,task_name,status,priority,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched\n";

fn write_habits(dir: &Path, body: &str) {
    let tasks = dir.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(tasks.join("habits.csv"), format!("{HEADER}{body}")).unwrap();
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 31).unwrap()
}

#[test]
fn name_matches_tolerates_word_reordering() {
    assert!(name_matches(
        "Send status update to team",
        "send team status update"
    ));
    assert!(name_matches(
        "Morning Inbox & Readings (10 mins)",
        "morning inbox"
    ));
    assert!(!name_matches("Meds", "meditate"));
}

#[test]
fn single_lapsed_match_respawns_and_writes() {
    let dir = tempfile::tempdir().unwrap();
    write_habits(
        dir.path(),
        "H1,Meds,done,p0,2026-07-24,1,days,2026-07-23,2026-07-25,2026-07-25\n\
             H2,Meds,done,p0,2026-07-26,1,days,2026-07-25,2026-07-27,2026-07-27\n",
    );
    let out = revive_fuzzy_in_root(dir.path(), "meds", today()).unwrap();
    assert_eq!(
        out,
        ReviveOutcome::Revived {
            name: "Meds".to_owned(),
            next_id: "H3".to_owned(),
            next_due: "2026-08-01".to_owned(),
        }
    );
    let written = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
    assert!(
        written.contains("H3,Meds,not_started,p0,2026-08-01,1,days,2026-07-31,,2026-07-31"),
        "spawned row missing; got:\n{written}"
    );
}

#[test]
fn healthy_habit_reports_no_action_and_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    write_habits(
        dir.path(),
        "H1,Meds,done,p0,2026-07-24,1,days,2026-07-23,2026-07-25,2026-07-25\n\
             H2,Meds,not_started,p0,2026-08-01,1,days,2026-07-25,,2026-07-25\n",
    );
    let before = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
    let out = revive_fuzzy_in_root(dir.path(), "meds", today()).unwrap();
    assert_eq!(out, ReviveOutcome::Healthy("Meds".to_owned()));
    let after = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
    assert_eq!(before, after, "healthy revive must not rewrite the file");
}

#[test]
fn ambiguous_match_lists_names_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    write_habits(
        dir.path(),
        "H1,Morning Inbox & Readings (10 mins),done,p2,2026-07-26,1,days,2026-07-25,2026-07-27,2026-07-27\n\
             H2,Morning Triage (5mins),done,p1,2026-07-26,1,days,2026-07-25,2026-07-27,2026-07-27\n",
    );
    let out = revive_fuzzy_in_root(dir.path(), "morning", today()).unwrap();
    assert_eq!(
        out,
        ReviveOutcome::Ambiguous(vec![
            "Morning Inbox & Readings (10 mins)".to_owned(),
            "Morning Triage (5mins)".to_owned(),
        ])
    );
}

#[test]
fn no_match_reports_nomatch() {
    let dir = tempfile::tempdir().unwrap();
    write_habits(
        dir.path(),
        "H1,Meds,done,p0,2026-07-26,1,days,2026-07-25,2026-07-27,2026-07-27\n",
    );
    let out = revive_fuzzy_in_root(dir.path(), "nonexistent chore", today()).unwrap();
    assert_eq!(out, ReviveOutcome::NoMatch);
}

#[test]
fn revive_named_anchors_to_latest_scheduled_instance() {
    // Rows out of order; the anchor must be the max `due_date`, not file order.
    let dir = tempfile::tempdir().unwrap();
    write_habits(
        dir.path(),
        "H5,Replace cat litter,done,p2,2026-07-15,3,days,2026-07-13,2026-07-27,2026-07-27\n\
             H1,Replace cat litter,done,p2,2026-07-12,3,days,2026-07-10,2026-07-12,2026-07-12\n",
    );
    let out = revive_named_in_root(dir.path(), "Replace cat litter", today()).unwrap();
    match out {
        ReviveOutcome::Revived { next_due, .. } => assert_eq!(next_due, "2026-08-02"),
        other => panic!("expected Revived, got {other:?}"),
    }
}
