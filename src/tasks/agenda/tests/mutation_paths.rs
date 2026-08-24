//! Every native path that writes `tasks.csv` or `habits.csv` must leave the
//! day's agenda accurate. One test per path, each driving the real mutator.

use super::fixture::{Fixture, actor, fixture};
use super::today;
use crate::tasks::agenda::Outcome;

/// The habits CSV with a weekly habit alongside the daily one, so the
/// cadence-dependent paths have something to work with.
const AGENDA_WITH_HABITS: &str = "\
# Monday 2026-08-24

**Load:** 2 tasks, 2 habits

## ❗ Most important

- [ ] ❗ **T535** Fix the sync (45m)

## Suggested order

1. [ ] 09:00 | **H304** Walk the dog
2. [ ] 10:00 | **T536** Write the docs (30m)
";

fn weekly_habit(fixture: &Fixture) {
    let path = fixture.targets.tasks_dir.join("habits.csv");
    let mut text = std::fs::read_to_string(&path).expect("read habits");
    text.push_str("H400,Water the plants,not_started,2026-08-24,09:00,1,weeks,,2026-08-20\n");
    std::fs::write(&path, text).expect("write habits");
}

fn agenda(fixture: &Fixture) -> String {
    std::fs::read_to_string(&fixture.targets.markdown).expect("read agenda")
}

#[test]
fn skipping_a_daily_habit_moves_it_into_the_completed_snapshot() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));

    let (_, outcome) = crate::tasks::skip::skip_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        "H304",
        None,
        today(),
    )
    .expect("skip");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    let synced = agenda(&fixture);
    // A daily skip *is* a completion, so the row leaves the plan and joins
    // the completed snapshot.
    assert!(!synced.contains("1. [ ] 09:00 | **H304**"), "{synced}");
    assert!(synced.contains("| ✅ **H304** Walk the dog |"), "{synced}");
}

#[test]
fn skipping_a_non_daily_habit_only_drops_it_from_the_plan() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));
    weekly_habit(&fixture);

    let (_, outcome) = crate::tasks::skip::skip_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        "H400",
        None,
        today(),
    )
    .expect("skip");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    let synced = agenda(&fixture);
    // Deferred, not done — so it must not appear as completed anywhere.
    assert!(!synced.contains("**H400**"), "{synced}");
}

/// Revive still runs the sync — every mutation does — but it is the one path
/// that provably cannot change today's agenda: the occurrence it spawns is
/// dated strictly *after* today, and the done row it revives was completed on
/// an earlier day. So the sync is a no-op, and asserting that is the point.
#[test]
fn reviving_a_lapsed_habit_leaves_today_alone() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));
    // Lapse the chain: the only occurrence is done, none pending.
    let path = fixture.targets.tasks_dir.join("habits.csv");
    let text = std::fs::read_to_string(&path)
        .expect("read habits")
        .replace(
            "H304,Walk the dog,not_started,2026-08-24",
            "H304,Walk the dog,done,2026-08-20",
        );
    std::fs::write(&path, text).expect("write habits");
    let before = agenda(&fixture);

    let (revived, outcome) = crate::tasks::revive::revive_named_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        "Walk the dog",
        today(),
    )
    .expect("revive");

    assert!(matches!(
        revived,
        crate::tasks::revive::ReviveOutcome::Revived { .. }
    ));
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(agenda(&fixture), before);
}

#[test]
fn completing_managed_triage_updates_the_agenda() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));
    let path = fixture.targets.tasks_dir.join("habits.csv");
    let mut text = std::fs::read_to_string(&path).expect("read habits");
    text = text.replace(
        "task_id,task_name,status,due_date,ideal_time,recur_interval,recur_unit,completed_date,last_touched",
        "task_id,task_name,status,due_date,ideal_time,recur_interval,recur_unit,completed_date,last_touched,system_key",
    );
    text.push_str(
        "H500,Morning Triage,not_started,2026-08-24,06:00,1,days,,2026-08-20,brain.triage.daily\n",
    );
    std::fs::write(&path, text).expect("write habits");

    let (_, outcome) = crate::tasks::triage_habits::complete_managed::complete_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        crate::tasks::triage_habits::ManagedTriageKind::Daily,
        true,
        today(),
    )
    .expect("complete managed triage");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    assert!(
        agenda(&fixture).contains("| ✅ **H500** Morning Triage |"),
        "{}",
        agenda(&fixture)
    );
}

#[test]
fn the_habits_page_completion_updates_the_agenda() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));

    let outcome = crate::server::routes::habits::complete_and_sync_in_root(
        &fixture.root,
        &fixture.root.join("tasks.transaction.lock"),
        &fixture.targets,
        r#"{"task_id": "H304"}"#,
        today(),
    );

    assert!(matches!(
        outcome,
        crate::server::routes::habits::DoneOutcome::Done { .. }
    ));
    assert!(
        agenda(&fixture).contains("| ✅ **H304** Walk the dog |"),
        "{}",
        agenda(&fixture)
    );
}

#[test]
fn adding_a_habit_due_today_puts_it_on_the_agenda() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));

    let (_, outcome) = crate::tasks::add::create_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        &actor(&fixture.root),
        &crate::tasks::add::CreateRequest {
            name: "Water the plants".to_owned(),
            priority: "p2".to_owned(),
            due: Some(today().to_string()),
            habit: true,
            interval: Some(1),
            unit: Some("days".to_owned()),
            ..Default::default()
        },
        today(),
    )
    .expect("add");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    assert!(
        agenda(&fixture).contains("Water the plants"),
        "{}",
        agenda(&fixture)
    );
}

#[test]
fn setting_a_task_done_drops_it_from_the_plan() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));

    let (_, outcome) = crate::tasks::set::set_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        "T535",
        &crate::tasks::set::Edit {
            status: Some("done".to_owned()),
            ..Default::default()
        },
        today(),
    )
    .expect("set");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    let synced = agenda(&fixture);
    assert!(!synced.contains("- [ ] ❗ **T535**"), "{synced}");
}

#[test]
fn setting_a_due_date_off_today_drops_it_from_the_plan() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));

    let (_, outcome) = crate::tasks::set::set_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        "T536",
        &crate::tasks::set::Edit {
            due: Some("2026-09-01".to_owned()),
            ..Default::default()
        },
        today(),
    )
    .expect("set");

    assert_eq!(outcome, Outcome::Updated { pdf: false });
    assert!(
        !agenda(&fixture).contains("**T536**"),
        "{}",
        agenda(&fixture)
    );
}

#[test]
fn an_ordinary_field_edit_only_refreshes_the_snapshots() {
    let fixture = fixture(Some(AGENDA_WITH_HABITS));

    crate::tasks::set::set_in_root_and_sync(
        &fixture.root,
        &fixture.targets,
        "T536",
        &crate::tasks::set::Edit {
            notes: Some("call first".to_owned()),
            ..Default::default()
        },
        today(),
    )
    .expect("set");

    // Renaming a note is not a statement that the row left today's plan.
    assert!(
        agenda(&fixture).contains("2. [ ] 10:00 | **T536** Write the docs (30m)"),
        "{}",
        agenda(&fixture)
    );
}
