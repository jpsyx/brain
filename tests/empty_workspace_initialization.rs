use std::process::{Command, Output};

use tempfile::TempDir;

struct Fixture {
    home: TempDir,
    config_home: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("isolated HOME"),
            config_home: tempfile::tempdir().expect("isolated XDG_CONFIG_HOME"),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .output()
            .expect("run brain")
    }
}

#[test]
fn first_tasks_command_initializes_an_empty_workspace() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert!(fixture.run(&["workspace", "create", "--root", family.to_str().unwrap()]).status.success());
    assert!(fixture
        .run(&[
            "workspace",
            "repair",
            "--local-user-id",
            "pablo",
            "-w",
            "family",
        ])
        .status
        .success());

    let output = fixture.run(&["tasks", "today", "--no-tui", "-w", "family"]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for path in [
        ".config/config.json",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "tasks/.tasks_next_id",
        "tasks/.habits_next_id",
        "projects/projects-lookup.csv",
        "resources/zotero-lookup.csv",
    ] {
        assert!(family.join(path).is_file(), "missing {path}");
    }
    for directory in ["projects", "areas", "resources", "archive", "tasks"] {
        assert!(family.join(directory).is_dir(), "missing {directory}");
    }
}
