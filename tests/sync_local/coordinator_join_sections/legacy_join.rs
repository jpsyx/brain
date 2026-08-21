#[test]
fn second_configured_legacy_machine_joins_current_remote_through_real_coordinator_and_rclone() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let fixture = Fixture::new();

    fixture.repair_b_until_clean();
    fixture.repair_a_until_clean();
    fixture.write_a_change();
    Fixture::assert_success(&fixture.migrate_a(), "migrate first machine");
    fixture.seed_a_high_counters();
    Fixture::assert_success(&fixture.add_a_task(), "allocate first-machine high task");
    Fixture::assert_success(&fixture.add_a_habit(), "allocate first-machine high habit");
    fixture.repair_a_until_clean();
    let first_uuid = task_rows(&fixture.remote)["T1"]["task_uuid"].clone();
    let ordinary_repair = fixture.run_b_until_task_schema_refusal();
    assert!(
        !ordinary_repair.status.success(),
        "ordinary repair unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ordinary_repair.stdout),
        String::from_utf8_lossy(&ordinary_repair.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ordinary_repair.stderr)
            .contains("remote task schema is Current, but local task schema is Legacy"),
        "{}",
        String::from_utf8_lossy(&ordinary_repair.stderr)
    );

    fixture.write_b_changes();
    Fixture::assert_success(&fixture.migrate_b(), "migrate second machine");

    let joined = task_rows(&fixture.root_b);
    assert_eq!(joined["T1"]["task_uuid"], first_uuid);
    assert_eq!(joined["T1"]["notes"], "first-machine-note");
    assert_eq!(joined["T1"]["status"], "waiting");
    assert_eq!(
        joined["T2"]["task_uuid"],
        legacy_task_uuid(workspace_id(), CsvKind::Tasks, "T2").to_string()
    );
    assert_eq!(joined["T2"]["task_name"], "Second-machine only");
    assert_eq!(
        std::fs::read_to_string(fixture.root_b.join("tasks/.tasks_next_id")).unwrap(),
        "8\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root_b.join("tasks/.habits_next_id")).unwrap(),
        "9\n"
    );
    Fixture::assert_success(
        &fixture.add_b_task(),
        "allocate second-machine task after join",
    );
    Fixture::assert_success(
        &fixture.add_b_habit(),
        "allocate second-machine habit after join",
    );
    let allocated_tasks = task_rows(&fixture.root_b);
    assert_eq!(
        allocated_tasks["T8"]["task_name"],
        "Second-machine new task"
    );
    let allocated_habits = habit_rows(&fixture.root_b);
    assert_eq!(
        allocated_habits["H9"]["task_name"],
        "Second-machine new habit"
    );
    assert!(!fixture.migration_journal_b().exists());

    fixture.repair_a_until_clean();
    fixture.repair_b_until_clean();
    fixture.repair_a_until_clean();
    for relative in ["tasks/tasks.csv", "tasks/habits.csv", "tasks/SCHEMA.json"] {
        assert_eq!(
            std::fs::read(fixture.root_a.join(relative)).unwrap(),
            std::fs::read(fixture.root_b.join(relative)).unwrap(),
            "machines did not converge for {relative}"
        );
        assert_eq!(
            std::fs::read(fixture.root_b.join(relative)).unwrap(),
            std::fs::read(fixture.remote.join(relative)).unwrap(),
            "remote did not converge for {relative}"
        );
    }
}

struct Fixture {
    _temporary: tempfile::TempDir,
    remote: PathBuf,
    root_a: PathBuf,
    root_b: PathBuf,
    home_a: PathBuf,
    home_b: PathBuf,
    config_a: PathBuf,
    config_b: PathBuf,
    bin: PathBuf,
    real_rclone: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let remote = temporary.path().join("remote");
        let root_a = temporary.path().join("machine-a/workspace");
        let root_b = temporary.path().join("machine-b/workspace");
        let home_a = temporary.path().join("machine-a/home");
        let home_b = temporary.path().join("machine-b/home");
        let config_a = temporary.path().join("machine-a/config");
        let config_b = temporary.path().join("machine-b/config");
        let bin = temporary.path().join("bin");
        for directory in [&remote, &root_a, &root_b, &home_a, &home_b, &bin] {
            std::fs::create_dir_all(directory).unwrap();
        }
        write_legacy_workspace(&root_a);
        write_legacy_workspace(&root_b);
        write_remote_legacy(&remote, &root_a);
        write_registry(&config_a, &root_a);
        write_registry(&config_b, &root_b);
        let real_rclone = find_rclone();
        write_rclone_shim(&bin.join("rclone"));
        Self {
            _temporary: temporary,
            remote,
            root_a,
            root_b,
            home_a,
            home_b,
            config_a,
            config_b,
            bin,
            real_rclone,
        }
    }

    fn migrate_a(&self) -> Output {
        self.run_a(&[
            "workspace",
            "migrate",
            "-b",
            "family",
            "--acknowledge-all-machines-updated",
        ])
    }

    fn migrate_b(&self) -> Output {
        self.run_b(&[
            "workspace",
            "migrate",
            "-b",
            "family",
            "--acknowledge-all-machines-updated",
        ])
    }

    fn run_a(&self, args: &[&str]) -> Output {
        self.run(args, &self.home_a, &self.config_a)
    }

    fn run_b(&self, args: &[&str]) -> Output {
        self.run(args, &self.home_b, &self.config_b)
    }

    fn run(&self, args: &[&str], home: &Path, config: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", config)
            .env("NO_COLOR", "1")
            .env("REMOTE_ROOT", &self.remote)
            .env("REAL_RCLONE", &self.real_rclone)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .output()
            .unwrap()
    }

    fn assert_success(output: &Output, phase: &str) {
        assert!(
            output.status.success(),
            "{phase} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_sync_complete(output: &Output, phase: &str) {
        Self::assert_success(output, phase);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("sync complete."),
            "{phase} did not complete cleanly\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repair_a_until_clean(&self) {
        let mut last = None;
        for _ in 0..3 {
            let output = self.run_a(&["sync", "repair", "-b", "family"]);
            if String::from_utf8_lossy(&output.stdout).contains("sync complete.") {
                return;
            }
            last = Some(output);
        }
        Self::assert_sync_complete(last.as_ref().unwrap(), "repair first-machine baseline");
    }

    fn repair_b_until_clean(&self) {
        let mut last = None;
        for _ in 0..3 {
            let output = self.run_b(&["sync", "repair", "-b", "family"]);
            if String::from_utf8_lossy(&output.stdout).contains("sync complete.") {
                return;
            }
            last = Some(output);
        }
        Self::assert_sync_complete(last.as_ref().unwrap(), "repair second-machine baseline");
    }

    fn run_b_until_task_schema_refusal(&self) -> Output {
        let mut last = None;
        for _ in 0..3 {
            let output = self.run_b(&["sync", "repair", "-b", "family"]);
            if String::from_utf8_lossy(&output.stderr)
                .contains("remote task schema is Current, but local task schema is Legacy")
            {
                return output;
            }
            last = Some(output);
        }
        last.expect("at least one second-machine repair attempt")
    }

    fn write_a_change(&self) {
        write_tasks(
            &self.root_a,
            "task_id,task_name,status,notes,assigned_to\n\
             T1,Shared,not_started,first-machine-note,pablo\n",
        );
    }

    fn seed_a_high_counters(&self) {
        std::fs::write(self.root_a.join("tasks/.tasks_next_id"), "7\n").unwrap();
        std::fs::write(self.root_a.join("tasks/.habits_next_id"), "8\n").unwrap();
    }

    fn add_a_task(&self) -> Output {
        Self::run_add(
            &self.root_a,
            &self.home_a,
            &["--name", "First-machine high task", "--type", "personal"],
        )
    }

    fn add_a_habit(&self) -> Output {
        Self::run_add(
            &self.root_a,
            &self.home_a,
            &[
                "--name",
                "First-machine high habit",
                "--habit",
                "--interval",
                "1",
                "--unit",
                "days",
            ],
        )
    }

    fn add_b_task(&self) -> Output {
        Self::run_add(
            &self.root_b,
            &self.home_b,
            &["--name", "Second-machine new task", "--type", "personal"],
        )
    }

    fn add_b_habit(&self) -> Output {
        Self::run_add(
            &self.root_b,
            &self.home_b,
            &[
                "--name",
                "Second-machine new habit",
                "--habit",
                "--interval",
                "1",
                "--unit",
                "days",
            ],
        )
    }

    fn run_add(root: &Path, home: &Path, args: &[&str]) -> Output {
        Command::new("python3")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/todo/scripts/add_task.py"))
            .args(args)
            .args(["--priority", "p2"])
            .env("HOME", home)
            .env("BRAIN_ROOT", root)
            .env("BRAIN_ACTOR_ID", "pablo")
            .env("BRAIN_WORKSPACE_ID", WORKSPACE_ID)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .unwrap()
    }

    fn write_b_changes(&self) {
        write_tasks(
            &self.root_b,
            "task_id,task_name,status,notes,assigned_to\n\
             T1,Shared,waiting,,pablo\n\
             T2,Second-machine only,not_started,,pablo\n",
        );
    }

    fn migration_journal_b(&self) -> PathBuf {
        self.home_b
            .join(".cache/brain/workspaces")
            .join(WORKSPACE_ID)
            .join("migrations/multi-workspace-v1.json")
    }
}

fn write_legacy_workspace(root: &Path) {
    WorkspaceManifest::new(workspace_id())
        .write_new(root)
        .unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        b"{\"schema_version\":1,\"users\":[{\"id\":\"pablo\",\"name\":\"Pablo\",\"phones\":[],\"emails\":[],\"response_email\":null}]}\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"access_mode\":\"unrestricted\",\"enable_triage_habits\":false}\n",
    )
    .unwrap();
    write_tasks(
        root,
        "task_id,task_name,status,notes,assigned_to\n\
         T1,Shared,not_started,,pablo\n",
    );
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_id,task_name,status,notes,assigned_to\nH1,Walk,not_started,,pablo\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), b"{}\n").unwrap();
    std::fs::write(root.join("tasks/.tasks_next_id"), b"2\n").unwrap();
    std::fs::write(root.join("tasks/.habits_next_id"), b"2\n").unwrap();
    std::fs::write(
        root.join("RCLONE_TEST"),
        b"brain sync check-access marker\n",
    )
    .unwrap();
    std::fs::write(root.join("stable.md"), b"unchanged portable file\n").unwrap();
}
