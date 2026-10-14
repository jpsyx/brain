use brain::workspace::{WorkspaceId, WorkspaceManifest, WorkspacePaths};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

pub struct Fixture {
    pub temp: TempDir,
    pub registry: Value,
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("config/brain/migrations")).unwrap();
        fs::write(
            temp.path().join("config/brain/migrations/version"),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        Self {
            temp,
            registry: json!({"schema_version":4,"default_workspace":"alpha","workspaces":{}}),
        }
    }
    pub fn root(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    pub fn add(&mut self, name: &str) -> PathBuf {
        let root = self.root(name);
        let id = WorkspaceId::new();
        WorkspaceManifest::new(id).write_new(&root).unwrap();
        fs::create_dir_all(root.join(".config/extensions")).unwrap();
        fs::write(
            root.join(".config/config.json"),
            r#"{"skills_auto_sync":false}"#,
        )
        .unwrap();
        fs::write(
            root.join(".config/extensions/todo.md"),
            format!("[todo:anchors]\n{name} extension\n"),
        )
        .unwrap();
        self.registry["workspaces"][name] =
            json!({"workspace_id":id,"root":root,"aliases":[],"local_user_id":"test-user"});
        self.save();
        root
    }
    pub fn save(&self) {
        fs::write(
            self.registry_path(),
            serde_json::to_vec(&self.registry).unwrap(),
        )
        .unwrap();
    }
    pub fn registry_path(&self) -> PathBuf {
        self.temp.path().join("config/brain/env.json")
    }
    pub fn cache(&self, name: &str) -> PathBuf {
        let id = WorkspaceId::parse(
            self.registry["workspaces"][name]["workspace_id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        WorkspacePaths::new(&self.temp.path().join("home"), id)
            .cache_dir()
            .to_owned()
    }
    pub fn command(&self) -> Command {
        fs::create_dir_all(self.temp.path().join("home")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
        command
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env_remove("BRAIN_WORKSPACE_ID")
            .env_remove("BRAIN_WORKSPACE")
            .env_remove("BRAIN_REQUIRE_WORKSPACE")
            .env("NO_COLOR", "1")
            .current_dir(self.temp.path());
        command
    }
    pub fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    pub fn sync(&self) -> Output {
        self.run(&["skills", "sync", "--all-workspaces", "--json"])
    }
}
pub fn document(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON {e}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
pub fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
pub fn todo(root: &Path) -> PathBuf {
    root.join(".agents/skills/todo/SKILL.md")
}
