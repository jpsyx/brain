use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;

const VERSION_START: &str = concat!("brain start ", env!("CARGO_PKG_VERSION"));

fn log_path(stdout: &str) -> &str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("verbose log: "))
        .expect("verbose log path is printed")
}

fn fake_markdown_to_pdf(path: &std::path::Path) {
    std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("fake markdown-to-pdf");
    let mut perms = std::fs::metadata(path)
        .expect("fake metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("fake executable");
}

fn write_env(
    config: &std::path::Path,
    markdown_to_pdf: &std::path::Path,
    root: Option<&std::path::Path>,
) {
    std::fs::create_dir_all(config.join("brain")).expect("config dir");
    let root = root.map_or_else(String::new, |root| {
        format!(",\"root\":\"{}\"", root.display())
    });
    std::fs::write(
        config.join("brain").join("env.json"),
        format!(
            "{{\"markdown_to_pdf_path\":\"{}\"{root}}}\n",
            markdown_to_pdf.display()
        ),
    )
    .expect("env json");
}

fn make_ready(home: &std::path::Path, config: &std::path::Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config)
        .env("NO_COLOR", "1")
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "test-user",
        ])
        .output()
        .expect("repair workspace readiness");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verbose_non_tui_commands_mirror_logs_and_print_the_log_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    make_ready(&home, &config);
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("NO_COLOR", "1")
        .args(["--verbose", "config", "get", "__missing_verbose_test_key__"])
        .output()
        .expect("run brain binary");

    assert!(
        output.status.success(),
        "brain --verbose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("verbose output is utf-8");
    assert!(stdout.contains(VERSION_START), "{stdout}");
    assert!(stdout.contains("dispatch config"), "{stdout}");

    let log_path = log_path(&stdout);
    assert!(log_path.starts_with("/tmp/"), "{log_path}");
    assert!(
        std::path::Path::new(log_path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log")),
        "{log_path}"
    );

    let file = std::fs::read_to_string(log_path).expect("log file exists");
    assert!(file.contains(VERSION_START), "{file}");
    assert!(file.contains("dispatch config"), "{file}");
    assert!(
        file.contains("config get name=__missing_verbose_test_key__"),
        "{file}"
    );
    let _ = std::fs::remove_file(log_path);
}

#[test]
fn verbose_no_tui_tasks_log_the_csv_load_and_render_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    let bin = temp.path().join("bin");
    let tasks_dir = temp.path().join("brain").join("tasks");
    std::fs::create_dir_all(&bin).expect("bin dir");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");

    let markdown_to_pdf = bin.join("markdown-to-pdf");
    fake_markdown_to_pdf(&markdown_to_pdf);
    write_env(&config, &markdown_to_pdf, None);
    make_ready(&home, &config);

    let tasks_csv = tasks_dir.join("tasks.csv");
    std::fs::write(
        &tasks_csv,
        "task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assignee,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n\
T1,Write logs,mit,not_started,p1,2026-07-27,false,,,,,,,,,,0,2026-07-26,,2026-07-26,\n",
    )
    .expect("tasks csv");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .args([
            "--verbose",
            "tasks",
            "--no-tui",
            "--csv",
            &tasks_csv.display().to_string(),
        ])
        .output()
        .expect("run brain binary");

    assert!(
        output.status.success(),
        "brain --verbose tasks failed: {}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stdout = String::from_utf8(output.stdout).expect("verbose output is utf-8");
    let log_path = log_path(&stdout);
    let file = std::fs::read_to_string(log_path).expect("log file exists");

    assert!(file.contains("tasks csv "), "{file}");
    assert!(file.contains(&tasks_csv.display().to_string()), "{file}");
    assert!(file.contains("habits csv "), "{file}");
    assert!(file.contains("build tasks view"), "{file}");
    assert!(file.contains("render tasks no-tui"), "{file}");

    let _ = std::fs::remove_file(log_path);
}

#[test]
fn verbose_complete_logs_the_root_id_and_csv_mutation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    let bin = temp.path().join("bin");
    let brain = temp.path().join("brain");
    let tasks_dir = brain.join("tasks");
    std::fs::create_dir_all(&bin).expect("bin dir");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");

    let markdown_to_pdf = bin.join("markdown-to-pdf");
    fake_markdown_to_pdf(&markdown_to_pdf);
    write_env(&config, &markdown_to_pdf, Some(&brain));
    make_ready(&home, &config);

    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assignee,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n\
T1,Ship logging,mit,not_started,p1,2026-07-27,false,,,,,,,,,,0,2026-07-26,,2026-07-26,\n",
    )
    .expect("tasks csv");
    std::fs::write(
        tasks_dir.join("habits.csv"),
        "task_id,task_name,status,priority,due_date,hard_deadline,assignee,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched\n",
    )
    .expect("habits csv");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .args(["--verbose", "tasks", "complete", "T1"])
        .output()
        .expect("run brain binary");

    assert!(
        output.status.success(),
        "brain --verbose complete failed: {}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stdout = String::from_utf8(output.stdout).expect("verbose output is utf-8");
    let log_path = log_path(&stdout);
    let file = std::fs::read_to_string(log_path).expect("log file exists");

    assert!(file.contains("tasks complete raw_id=T1"), "{file}");
    assert!(
        file.contains(&format!("complete root {}", brain.display())),
        "{file}"
    );
    assert!(file.contains("complete normalized_id=T1"), "{file}");
    assert!(file.contains("write tasks csv"), "{file}");
    assert!(file.contains("complete result kind=Task id=T1"), "{file}");

    let _ = std::fs::remove_file(log_path);
}
