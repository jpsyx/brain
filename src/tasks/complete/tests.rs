use super::{
    CompletionKind, complete_in_root_for_actor_with_today, complete_in_root_with_today,
    normalize_id,
};
use chrono::NaiveDate;

fn local_actor(root: &std::path::Path) -> crate::actor::ActorContext {
    let workspace = crate::workspace::WorkspaceContext::new(
        root,
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        crate::workspace::WorkspaceName::parse("legacy").unwrap(),
        root,
        "pablo",
        root,
    )
    .unwrap();
    crate::actor::local_actor(&workspace).unwrap()
}

#[test]
fn command_runner_requires_explicit_workspace_and_actor_contexts() {
    fn accepts_runner(
        _: fn(
            &crate::workspace::WorkspaceContext,
            &str,
            &crate::actor::ActorContext,
        ) -> anyhow::Result<()>,
    ) {
    }
    accepts_runner(super::run);
}

#[test]
fn bare_number_assumes_task_prefix() {
    assert_eq!(normalize_id("123").unwrap(), "T123");
}

#[test]
fn lowercase_t_becomes_uppercase() {
    assert_eq!(normalize_id("t42").unwrap(), "T42");
}

#[test]
fn lowercase_h_becomes_uppercase() {
    assert_eq!(normalize_id("h7").unwrap(), "H7");
}

#[test]
fn leading_zeros_are_stripped() {
    assert_eq!(normalize_id("T00123").unwrap(), "T123");
    assert_eq!(normalize_id("h007").unwrap(), "H7");
}

#[test]
fn empty_input_errors() {
    assert!(normalize_id("").is_err());
    assert!(normalize_id("   ").is_err());
}

#[test]
fn non_digit_after_prefix_errors() {
    assert!(normalize_id("Tfoo").is_err());
    assert!(normalize_id("h-1").is_err());
}

#[test]
fn completing_a_task_marks_done_and_touched_in_tasks_csv() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,task_type,status,completed_date,last_touched,project,linear_issue\n\
             T1,Ship native complete,mit,not_started,,,alpha,LIN-1\n",
    )
    .unwrap();
    std::fs::write(
            tasks_dir.join("habits.csv"),
            "task_id,task_name,status,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched\n",
        )
        .unwrap();

    let result = complete_in_root_for_actor_with_today(
        dir.path(),
        "T1",
        NaiveDate::from_ymd_opt(2026, 7, 26).unwrap(),
        &local_actor(dir.path()),
    )
    .unwrap();

    assert_eq!(result.kind, CompletionKind::Task);
    let written = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
    assert!(written.contains("T1,Ship native complete,mit,done,2026-07-26,2026-07-26,alpha,LIN-1"));
}

#[test]
fn completing_by_display_id_preserves_the_immutable_task_uuid() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let task_uuid = "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4";
    std::fs::write(
            tasks_dir.join("tasks.csv"),
            format!(
                "task_uuid,task_id,task_name,status,completed_date,last_touched\n{task_uuid},T91,Preserve identity,not_started,,\n"
            ),
        )
        .unwrap();
    std::fs::write(
        tasks_dir.join("habits.csv"),
        "task_uuid,task_id,task_name,status,completed_date,last_touched\n",
    )
    .unwrap();

    complete_in_root_with_today(
        dir.path(),
        "T91",
        NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
    )
    .unwrap();

    let written = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
    assert!(written.contains(&format!("{task_uuid},T91,Preserve identity,done")));
}

#[test]
fn unrelated_mutation_migrates_assignee_header_and_preserves_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,status,assignee,completed_date,last_touched\n\
T1,Preserve owner,not_started,wife,,\n",
    )
    .unwrap();
    std::fs::write(
        tasks_dir.join("habits.csv"),
        "task_id,task_name,status,assigned_to,completed_date,last_touched\n",
    )
    .unwrap();

    complete_in_root_with_today(
        dir.path(),
        "T1",
        NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
    )
    .unwrap();

    let written = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
    assert_eq!(
        written,
        "task_id,task_name,status,assigned_to,completed_date,last_touched\n\
T1,Preserve owner,done,wife,2026-08-03,2026-08-03\n"
    );
}

#[test]
fn writer_prefers_canonical_assignment_when_both_headers_exist() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,status,assignee,assigned_to,completed_date,last_touched\n\
T1,Keep canonical,not_started,legacy,wife,,\n",
    )
    .unwrap();
    std::fs::write(
        tasks_dir.join("habits.csv"),
        "task_id,task_name,status,assigned_to,completed_date,last_touched\n",
    )
    .unwrap();

    complete_in_root_with_today(
        dir.path(),
        "T1",
        NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
    )
    .unwrap();

    let written = std::fs::read_to_string(tasks_dir.join("tasks.csv")).unwrap();
    assert_eq!(
        written,
        "task_id,task_name,status,assigned_to,completed_date,last_touched\n\
T1,Keep canonical,done,wife,2026-08-03,2026-08-03\n"
    );
}

#[test]
fn completing_a_habit_spawns_the_next_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,status,completed_date,last_touched\n",
    )
    .unwrap();
    std::fs::write(tasks_dir.join(".habits_next_id"), "2\n").unwrap();
    std::fs::write(
            tasks_dir.join("habits.csv"),
            "task_id,task_name,status,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched\n\
             H1,Morning pages,not_started,2026-07-24,1,days,2026-07-24,,\n",
        )
        .unwrap();

    let result = complete_in_root_with_today(
        dir.path(),
        "H1",
        NaiveDate::from_ymd_opt(2026, 7, 26).unwrap(),
    )
    .unwrap();

    assert_eq!(result.kind, CompletionKind::Habit);
    assert_eq!(result.next_due.as_deref(), Some("2026-07-27"));
    let written = std::fs::read_to_string(tasks_dir.join("habits.csv")).unwrap();
    assert_eq!(
        written.lines().next().unwrap_or_default().split(',').next(),
        Some("task_id"),
        "legacy sync stays keyed by task_id until coordinated migration"
    );
    assert!(
        written
            .contains("H1,Morning pages,done,2026-07-24,1,days,2026-07-24,2026-07-26,2026-07-26")
    );
    assert!(written.contains("H2,Morning pages,not_started,2026-07-27,1,days,2026-07-26,,"));
    assert_eq!(
        std::fs::read_to_string(tasks_dir.join(".habits_next_id")).unwrap(),
        "3\n"
    );
}

#[test]
fn spawned_habit_gets_new_uuid_and_retains_system_key_and_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_uuid,task_id,task_name,status,completed_date,last_touched\n",
    )
    .unwrap();
    std::fs::write(tasks_dir.join(".habits_next_id"), "2\n").unwrap();
    let source_uuid = "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4";
    std::fs::write(
            tasks_dir.join("habits.csv"),
            format!(
                "task_uuid,task_id,task_name,status,assigned_to,system_key,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched\n{source_uuid},H1,Morning triage,not_started,wife,brain.triage.daily,2026-08-03,1,days,2026-08-03,,\n"
            ),
        )
        .unwrap();

    complete_in_root_with_today(
        dir.path(),
        "H1",
        NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
    )
    .unwrap();

    let mut reader = csv::Reader::from_path(tasks_dir.join("habits.csv")).unwrap();
    let rows = reader
        .deserialize::<std::collections::BTreeMap<String, String>>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        uuid::Uuid::parse_str(&rows[1]["task_uuid"])
            .unwrap()
            .get_version_num(),
        4
    );
    assert_ne!(rows[1]["task_uuid"], source_uuid);
    assert_eq!(rows[1]["system_key"], "brain.triage.daily");
    assert_eq!(rows[1]["assigned_to"], "wife");
}

/// The recurrence contract for a late completion: the cadence ladder is
/// anchored to the row's own `due_date`, and the next occurrence is the first
/// rung **strictly after** today. So a late completion keeps the original
/// cadence even when that lands the next occurrence as soon as tomorrow, and a
/// rung falling exactly on today is skipped for a full further interval.
mod recurrence_anchor {
    use super::super::next_due;
    use chrono::{Datelike, NaiveDate};

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn keeps_the_cadence_when_late_even_if_the_next_rung_is_tomorrow() {
        // 3-day habit due Aug 2, completed 2 days late on Aug 4: the ladder's
        // next rung is Aug 5, and cadence wins over "a full interval away".
        assert_eq!(
            next_due("2026-08-02", 3, "days", day("2026-08-04")).unwrap(),
            "2026-08-05"
        );
    }

    #[test]
    fn skips_a_rung_that_lands_exactly_on_today() {
        // Daily habit due Aug 6 completed Aug 7: the Aug 7 rung is today, so
        // the next occurrence is a full interval past it.
        assert_eq!(
            next_due("2026-08-06", 1, "days", day("2026-08-07")).unwrap(),
            "2026-08-08"
        );
        // Same rule at a 3-day cadence: Aug 2 + 3 = Aug 5 is today → Aug 8.
        assert_eq!(
            next_due("2026-08-02", 3, "days", day("2026-08-05")).unwrap(),
            "2026-08-08"
        );
    }

    #[test]
    fn preserves_the_weekday_of_a_stale_weekly_habit() {
        // Monday-weekly habit 8 weeks stale still lands on a Monday.
        let next = next_due("2026-06-01", 1, "weeks", day("2026-07-29")).unwrap();
        assert_eq!(next, "2026-08-03");
        assert_eq!(day(&next).weekday(), chrono::Weekday::Mon);
    }

    #[test]
    fn preserves_the_day_of_month_for_a_stale_monthly_habit() {
        assert_eq!(
            next_due("2026-04-07", 1, "months", day("2026-08-07")).unwrap(),
            "2026-09-07"
        );
    }

    #[test]
    fn an_on_time_completion_advances_exactly_one_interval() {
        assert_eq!(
            next_due("2026-08-07", 1, "days", day("2026-08-07")).unwrap(),
            "2026-08-08"
        );
        assert_eq!(
            next_due("2026-08-07", 1, "weeks", day("2026-08-07")).unwrap(),
            "2026-08-14"
        );
    }

    #[test]
    fn an_early_completion_keeps_the_scheduled_anchor() {
        // Due Aug 10, completed Aug 7: the ladder still runs from Aug 10.
        assert_eq!(
            next_due("2026-08-10", 1, "weeks", day("2026-08-07")).unwrap(),
            "2026-08-17"
        );
    }
}
