use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceId, WorkspaceManifest,
    WorkspaceName, WorkspaceRecord,
};
use serde_json::{Value, json};

pub struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        for directory in [
            "tasks",
            "projects",
            "areas",
            "resources",
            "archive",
            ".config",
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let id = WorkspaceId::parse("7a92e17d-9a16-4355-9dfd-762908ed6794").unwrap();
        WorkspaceManifest::new(id).write_new(&root).unwrap();
        std::fs::write(root.join(".config/users.json"), r#"{"schema_version":1,"users":[{"id":"tester","name":"Tester","phones":[],"emails":[],"response_email":null}]}"#).unwrap();
        let name = WorkspaceName::parse("sample").unwrap();
        RegistryStore::from_path(temp.path().join("config/brain/env.json"))
            .replace(&MachineRegistry {
                schema_version: REGISTRY_SCHEMA_VERSION,
                default_workspace: name.clone(),
                workspaces: BTreeMap::from([(
                    name,
                    WorkspaceRecord {
                        workspace_id: id,
                        root: root.clone(),
                        aliases: BTreeSet::new(),
                        local_user_id: "tester".into(),
                        receiver_enabled: false,
                        env: serde_json::from_value(
                            json!({"agenda_markdown_dir": temp.path().join("agendas")}),
                        )
                        .unwrap(),
                    },
                )]),
                env: serde_json::from_value(json!({"markdown_to_pdf_path": "/usr/bin/true"}))
                    .unwrap(),
            })
            .unwrap();
        let fixture = Self { temp, root };
        let warm = fixture.run(&["brain", "-w", "sample", "tasks", "linked", "--json"]);
        assert!(
            warm.status.success(),
            "{}",
            String::from_utf8_lossy(&warm.stderr)
        );
        std::fs::write(
            fixture.root.join("tasks/tasks.csv"),
            "task_id,task_name,status,last_touched\nT17,Example task,not_started,2026-10-14\n",
        )
        .unwrap();
        std::fs::write(
            fixture.root.join("tasks/habits.csv"),
            "task_id,task_name,status,last_touched\n",
        )
        .unwrap();
        fixture
    }

    fn run(&self, argv: &[&str]) -> Output {
        assert_eq!(argv[0], "brain");
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(&argv[1..])
            .current_dir(self.temp.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_CACHE_HOME", self.temp.path().join("cache"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    pub fn json(&self, argv: &[&str], code: i32) -> Value {
        let output = self.run(argv);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{argv:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("one JSON stdout document")
    }

    pub fn canonical_bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        walkdir::WalkDir::new(&self.root)
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| {
                entry.file_type().is_file()
                    && (entry.path().starts_with(self.root.join("tasks"))
                        || entry.path().starts_with(self.root.join("projects")))
            })
            .map(|entry| {
                (
                    entry.path().to_path_buf(),
                    std::fs::read(entry.path()).unwrap(),
                )
            })
            .collect()
    }
}
