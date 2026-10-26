use super::*;
use rusqlite::OptionalExtension as _;
use std::os::unix::fs::PermissionsExt;

pub(super) struct Transfer {
    pub fixture: Fixture,
    pub remote: PathBuf,
    bin: PathBuf,
    pub trace: PathBuf,
}

impl Transfer {
    pub fn new() -> Self {
        let fixture = Fixture::new("0.105.3");
        write_json(
            &fixture.root.join(".config/config.json"),
            &json!({"skills_auto_sync": false}),
        );
        fixture.startup();
        write_json(
            &fixture.root.join(".config/users.json"),
            &json!({"schema_version": 1, "users": [{"id":"member", "name":"Member", "phones":[], "emails":[]}]}),
        );
        let registry_path = fixture.home.join(".config/brain/env.json");
        let mut registry = read_json(&registry_path);
        registry["workspaces"]["alpha"]["env"]["sync"] = json!({"enabled":true,"b2_bucket":"fixture","b2_path":"","b2_key_id":"fixture","b2_app_key":"fixture","watch":false});
        write_json(&registry_path, &registry);
        let remote = fixture.temp.path().join("remote");
        std::fs::create_dir_all(remote.join(".config/extensions")).unwrap();
        std::fs::write(
            remote.join(".config/extensions/workspace.md"),
            "[workspace:context]\nArrived together.\n",
        )
        .unwrap();
        brain::skills::install::sync(
            &brain::skills::layout::Layout::under_root(&remote),
            &brain::skills::install::Sources {
                extensions_dir: Some(remote.join(".config/extensions")),
                plugins_dir: None,
            },
        )
        .unwrap();
        std::fs::copy(
            fixture.root.join(".config/workspace.json"),
            remote.join(".config/workspace.json"),
        )
        .unwrap();
        for root in [&fixture.root, &remote] {
            std::fs::create_dir_all(root.join("tasks")).unwrap();
            std::fs::write(
                root.join("tasks/SCHEMA.json"),
                include_bytes!("../../src/tasks/schema/task_schema.json"),
            )
            .unwrap();
            for name in ["tasks.csv", "habits.csv"] {
                std::fs::write(
                    root.join("tasks").join(name),
                    "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
                )
                .unwrap();
            }
        }
        let bin = fixture.temp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let executable = bin.join("rclone");
        std::fs::write(&executable, include_bytes!("sync_transport.py")).unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let trace = fixture.temp.path().join("transport.jsonl");
        Self {
            fixture,
            remote,
            bin,
            trace,
        }
    }

    pub fn pull(&self, mode: &str) -> Output {
        self.run(&["sync", "--pull", "-w", "alpha"], mode)
    }

    pub fn run(&self, args: &[&str], mode: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", &self.fixture.home)
            .env("XDG_CONFIG_HOME", self.fixture.home.join(".config"))
            .env(
                "PATH",
                format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("BRAIN_FIXTURE_ROOT", &self.fixture.root)
            .env("BRAIN_FIXTURE_REMOTE", &self.remote)
            .env("BRAIN_FIXTURE_TRACE", &self.trace)
            .env(
                "BRAIN_FIXTURE_JOURNAL",
                self.fixture.cache().join("sync/journal.db"),
            )
            .env("BRAIN_FIXTURE_RECEIVE", mode)
            .env_remove("BRAIN_WORKSPACE")
            .env_remove("BRAIN_WORKSPACE_ID")
            .current_dir(self.fixture.temp.path())
            .output()
            .unwrap()
    }

    pub fn phase(&self) -> Option<String> {
        let db = rusqlite::Connection::open(self.fixture.cache().join("sync/journal.db")).unwrap();
        db.query_row(
            "SELECT phase FROM portable_receive WHERE slot=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    pub fn transfers(&self) -> usize {
        std::fs::read_to_string(&self.trace)
            .unwrap_or_default()
            .lines()
            .filter(|line| serde_json::from_str::<Vec<String>>(line).unwrap()[0] == "bisync")
            .count()
    }
}
