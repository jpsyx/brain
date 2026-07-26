use std::process::Command;

use chrono::Local;

fn script(name: &str) -> String {
    format!("skills/todo/scripts/{name}")
}

#[test]
fn defer_habit_stamps_last_touched_on_the_mutated_row() {
    let home = tempfile::tempdir().unwrap();
    let tasks_dir = home.path().join("brain/tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let habits = tasks_dir.join("habits.csv");
    std::fs::write(
        &habits,
        "task_id,task_name,status,priority,due_date,hard_deadline,assignee,see_also,notes,project,energy_level,context,estimated_duration,recur_interval,recur_unit,created_date,completed_date,last_touched\n\
H1,Stretch,not_started,p2,2026-07-20,false,me,,,,,,,1,days,2026-07-01,,2026-07-01\n",
    )
    .unwrap();

    let output = Command::new("python3")
        .arg(script("defer_habit.py"))
        .arg("H1")
        .env("HOME", home.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = std::fs::read_to_string(habits).unwrap();
    let today = Local::now().date_naive().to_string();
    let row = updated.lines().nth(1).unwrap_or_default();
    assert!(
        row.ends_with(&today),
        "mutated habit should be stamped with today's last_touched:\n{updated}"
    );
}

#[test]
fn defer_habit_adds_last_touched_to_legacy_habit_files() {
    let home = tempfile::tempdir().unwrap();
    let tasks_dir = home.path().join("brain/tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let habits = tasks_dir.join("habits.csv");
    std::fs::write(
        &habits,
        "task_id,task_name,status,priority,due_date,hard_deadline,assignee,see_also,notes,project,energy_level,context,estimated_duration,recur_interval,recur_unit,created_date,completed_date\n\
H1,Stretch,not_started,p2,2026-07-20,false,me,,,,,,,1,days,2026-07-01,\n",
    )
    .unwrap();

    let output = Command::new("python3")
        .arg(script("defer_habit.py"))
        .arg("H1")
        .env("HOME", home.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = std::fs::read_to_string(habits).unwrap();
    let mut lines = updated.lines();
    assert!(
        lines.next().unwrap_or_default().ends_with(",last_touched"),
        "legacy habit files should gain last_touched:\n{updated}"
    );
    assert!(
        lines
            .next()
            .unwrap_or_default()
            .ends_with(&Local::now().date_naive().to_string()),
        "mutated legacy habit row should be stamped:\n{updated}"
    );
}

#[test]
fn add_habit_writes_last_touched_for_new_habit_files() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("brain/tasks")).unwrap();

    let output = Command::new("python3")
        .arg(script("add_task.py"))
        .arg("--habit")
        .arg("--name")
        .arg("Stretch")
        .arg("--priority")
        .arg("p2")
        .arg("--interval")
        .arg("1")
        .arg("--unit")
        .arg("days")
        .env("HOME", home.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = std::fs::read_to_string(home.path().join("brain/tasks/habits.csv")).unwrap();
    let mut lines = updated.lines();
    assert!(
        lines.next().unwrap_or_default().ends_with(",last_touched"),
        "new habit files should include last_touched:\n{updated}"
    );
    assert!(
        lines
            .next()
            .unwrap_or_default()
            .ends_with(&Local::now().date_naive().to_string()),
        "new habit rows should be stamped:\n{updated}"
    );
}

#[test]
fn sync_rules_fix_stamps_rows_it_repairs() {
    let home = tempfile::tempdir().unwrap();
    let tasks_dir = home.path().join("brain/tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let tasks = tasks_dir.join("tasks.csv");
    std::fs::write(
        &tasks,
        "task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assignee,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n\
T1,Ship fix,code,done,,p1,,false,,me,,,,,,,,'',2026-07-01,,2026-07-01,\n",
    )
    .unwrap();
    std::fs::write(
        tasks_dir.join("habits.csv"),
        "task_id,task_name,status,priority,due_date,hard_deadline,assignee,see_also,notes,project,energy_level,context,estimated_duration,recur_interval,recur_unit,created_date,completed_date,last_touched\n",
    )
    .unwrap();

    let output = Command::new("python3")
        .arg(script("apply_sync_rules.py"))
        .arg("--fix")
        .env("HOME", home.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = std::fs::read_to_string(tasks).unwrap();
    let today = Local::now().date_naive().to_string();
    assert!(
        updated
            .lines()
            .nth(1)
            .unwrap_or_default()
            .ends_with(&format!("{today},")),
        "repaired task row should be stamped with today's last_touched:\n{updated}"
    );
}
