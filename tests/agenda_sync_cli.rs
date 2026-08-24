//! End-to-end: a native completion through the real binary leaves the day's
//! agenda markdown accurate (BR-19).

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

const TASKS_HEADER: &str = "task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assignee,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue";
const HABITS_HEADER: &str = "task_id,task_name,status,priority,due_date,hard_deadline,assignee,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched";

struct Workspace {
    _temporary: tempfile::TempDir,
    home: std::path::PathBuf,
    config: std::path::PathBuf,
    agenda_dir: std::path::PathBuf,
    tasks_dir: std::path::PathBuf,
}

impl Workspace {
    fn brain(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
        command
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("NO_COLOR", "1");
        command
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self.brain().args(args).output().expect("run brain");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "brain {args:?} failed:\n{stderr}");
        stderr
    }

    fn agenda(&self, today: &str) -> std::path::PathBuf {
        self.agenda_dir.join(format!("{today}.md"))
    }
}

fn executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write script");
    let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod");
}

fn workspace() -> Workspace {
    let temporary = tempfile::tempdir().expect("tempdir");
    let home = temporary.path().join("home");
    let config = temporary.path().join("config");
    let root = temporary.path().join("brain");
    let tasks_dir = root.join("tasks");
    let bin = temporary.path().join("bin");
    let agenda_dir = temporary.path().join("agenda");
    for directory in [&tasks_dir, &bin, &agenda_dir, &home] {
        std::fs::create_dir_all(directory).expect("create directory");
    }
    let renderer = bin.join("markdown-to-pdf");
    executable(&renderer, "#!/bin/sh\nexit 0\n");
    std::fs::create_dir_all(config.join("brain")).expect("config dir");
    std::fs::write(
        config.join("brain/env.json"),
        format!(
            "{{\"markdown_to_pdf_path\":\"{}\",\"root\":\"{}\"}}\n",
            renderer.display(),
            root.display()
        ),
    )
    .expect("env json");

    let workspace = Workspace {
        _temporary: temporary,
        home,
        config,
        agenda_dir,
        tasks_dir,
    };
    workspace.run(&[
        "workspace",
        "repair",
        "--manifest",
        "--local-user-id",
        "test-user",
    ]);
    // The default is the machine-global `/tmp`; a test must never write there.
    workspace.run(&[
        "env",
        "set",
        &format!("agenda_markdown_dir={}", workspace.agenda_dir.display()),
    ]);
    workspace
}

fn seed_csvs(workspace: &Workspace, today: &str) {
    std::fs::write(
        workspace.tasks_dir.join("tasks.csv"),
        format!(
            "{TASKS_HEADER}\n\
             T1,Fix the sync,mit,not_started,p1,{today},false,,,,,,,,45,,0,{today},,{today},\n\
             T2,Write the docs,,not_started,p2,{today},false,,,,,,,,30,,0,{today},,{today},\n"
        ),
    )
    .expect("tasks csv");
    std::fs::write(
        workspace.tasks_dir.join("habits.csv"),
        format!(
            "{HABITS_HEADER}\n\
             H1,Walk the dog,not_started,p2,{today},false,,,,,,,10,07:00,1,days,{today},,{today}\n"
        ),
    )
    .expect("habits csv");
}

fn seed_agenda(workspace: &Workspace, today: &str) {
    std::fs::write(
        workspace.agenda(today),
        format!(
            "# {today} — agenda\n\
             \n\
             **Load:** 2 tasks, 1 habit\n\
             **Bottom line:** ship the sync.\n\
             \n\
             ## ❗ MITs\n\
             \n\
             - [ ] ❗ **T1** Fix the sync (45m)\n\
             \n\
             ## Suggested order\n\
             \n\
             1. [ ] 09:00 | **T1** Fix the sync (45m)\n\
             2. [ ] 10:00 | **T2** Write the docs (30m)\n\
             \n\
             ## Cut order\n\
             \n\
             1. **T2** Write the docs\n\
             2. **T1** Fix the sync\n\
             \n\
             ## Notes to self\n\
             \n\
             Brain has never heard of this section.\n"
        ),
    )
    .expect("agenda");
}

#[test]
fn brain_tasks_complete_syncs_the_days_agenda() {
    let workspace = workspace();
    let today = chrono::Local::now().date_naive().to_string();
    seed_csvs(&workspace, &today);
    seed_agenda(&workspace, &today);

    let report = workspace.run(&["tasks", "complete", "T1"]);

    assert!(report.contains("agenda updated"), "{report}");
    let synced = std::fs::read_to_string(workspace.agenda(&today)).expect("read agenda");
    // The completed task leaves every actionable section, and the survivors
    // are renumbered from 1.
    assert!(!synced.contains("- [ ] ❗ **T1**"), "{synced}");
    assert!(
        synced.contains("1. [ ] 10:00 | **T2** Write the docs (30m)"),
        "{synced}"
    );
    assert!(synced.contains("1. **T2** Write the docs"), "{synced}");
    assert!(!synced.contains("2. **T1** Fix the sync"), "{synced}");
    // The snapshots are re-derived from the CSVs.
    assert!(synced.contains("| ◻ **H1** Walk the dog |"), "{synced}");
    assert!(synced.contains("| ✅ **T1** Fix the sync |"), "{synced}");
    // Everything else survives, including a section brain knows nothing about.
    assert!(synced.contains(&format!("# {today} — agenda")), "{synced}");
    assert!(synced.contains("**Load:** 2 tasks, 1 habit"), "{synced}");
    assert!(
        synced.contains("**Bottom line:** ship the sync."),
        "{synced}"
    );
    assert!(
        synced.contains("Brain has never heard of this section."),
        "{synced}"
    );
}

#[test]
fn sync_agenda_is_idempotent_and_reports_what_it_did() {
    let workspace = workspace();
    let today = chrono::Local::now().date_naive().to_string();
    seed_csvs(&workspace, &today);
    seed_agenda(&workspace, &today);

    let first = workspace.run(&["tasks", "sync-agenda", "T1", "--action", "defer"]);
    let after_first = std::fs::read_to_string(workspace.agenda(&today)).expect("read agenda");
    let second = workspace.run(&["tasks", "sync-agenda", "T1", "--action", "defer"]);
    let after_second = std::fs::read_to_string(workspace.agenda(&today)).expect("read agenda");

    assert!(
        first.contains(&format!("Synced the {today} agenda")),
        "{first}"
    );
    assert!(!after_first.contains("**T1**"), "{after_first}");
    assert!(
        second.contains(&format!("The {today} agenda was already accurate")),
        "{second}"
    );
    assert_eq!(after_first, after_second);
}

#[test]
fn a_day_without_an_agenda_still_completes_the_task() {
    let workspace = workspace();
    let today = chrono::Local::now().date_naive().to_string();
    seed_csvs(&workspace, &today);

    workspace.run(&["tasks", "complete", "T1"]);

    let tasks = std::fs::read_to_string(workspace.tasks_dir.join("tasks.csv")).expect("tasks csv");
    assert!(tasks.contains("T1,Fix the sync,mit,done"), "{tasks}");
    assert!(!workspace.agenda(&today).exists());
}
