use std::process::Command;

use chrono::Local;

fn script(name: &str) -> String {
    format!("skills/todo/scripts/{name}")
}

fn script_command(name: &str, home: &std::path::Path, root: &std::path::Path) -> Command {
    let xdg = home.join("xdg");
    std::fs::create_dir_all(xdg.join("brain")).unwrap();
    std::fs::write(xdg.join("brain/env.json"), "{}\n").unwrap();
    let mut command = Command::new("python3");
    command
        .arg(script(name))
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("BRAIN_ROOT", root)
        .env("BRAIN_WORKSPACE", "test")
        .env("BRAIN_WORKSPACE_ID", "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .env("BRAIN_ACTOR_ID", "tester")
        .env("PYTHONDONTWRITEBYTECODE", "1");
    command
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

    let output = script_command("defer_habit.py", home.path(), &home.path().join("brain"))
        .arg("H1")
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

    let output = script_command("defer_habit.py", home.path(), &home.path().join("brain"))
        .arg("H1")
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

    let output = script_command("add_task.py", home.path(), &home.path().join("brain"))
        .arg("--habit")
        .arg("--name")
        .arg("Stretch")
        .arg("--priority")
        .arg("p2")
        .arg("--interval")
        .arg("1")
        .arg("--unit")
        .arg("days")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let habits = home.path().join("brain/tasks/habits.csv");
    let updated = std::fs::read_to_string(&habits).unwrap();
    let mut reader = csv::Reader::from_path(habits).unwrap();
    let headers = reader.headers().unwrap().clone();
    let last_touched = headers.iter().position(|header| header == "last_touched");
    assert!(
        last_touched.is_some(),
        "new habit files should include last_touched:\n{updated}"
    );
    let record = reader.records().next().unwrap().unwrap();
    assert!(
        record.get(last_touched.unwrap()) == Some(&Local::now().date_naive().to_string()),
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

    let output = script_command(
        "apply_sync_rules.py",
        home.path(),
        &home.path().join("brain"),
    )
    .arg("--fix")
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

#[test]
fn sync_rules_honors_the_selected_brain_root() {
    let home = tempfile::tempdir().unwrap();
    let family = home.path().join("family");
    let tasks_dir = family.join("tasks");
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

    let output = script_command("apply_sync_rules.py", home.path(), &family)
        .arg("--fix")
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
        "selected-root task row was not repaired:\n{updated}"
    );
}

/// The `/todo` mutators no longer carry their own copy of the agenda-sync
/// logic: they hand the mutated id to `brain tasks sync-agenda`, the same
/// implementation brain's native completion runs in-process. `BRAIN_BIN`
/// records the invocation instead of running the real binary.
#[test]
fn mutator_scripts_delegate_the_agenda_sync_to_the_brain_binary() {
    use std::os::unix::fs::PermissionsExt as _;

    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("brain");
    let tasks_dir = root.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,status,priority,due_date,created_date,last_touched\n\
T1,Ship fix,not_started,p1,2026-07-20,2026-07-01,2026-07-01\n",
    )
    .unwrap();

    let recorded = home.path().join("argv.txt");
    let fake_brain = home.path().join("fake-brain");
    std::fs::write(
        &fake_brain,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n",
            recorded.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake_brain, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = script_command("touch_task.py", home.path(), &root)
        .env("BRAIN_BIN", &fake_brain)
        .arg("T1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let argv = std::fs::read_to_string(&recorded).unwrap_or_default();
    assert_eq!(
        argv.trim(),
        "-b test tasks sync-agenda T1 --action touch",
        "the mutator must delegate to the binary, naming the mutated workspace"
    );
}
