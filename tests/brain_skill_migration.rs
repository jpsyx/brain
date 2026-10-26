use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const ID: &str = "11111111-1111-4111-8111-111111111111";

struct Fixture {
    temp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new(version: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let root = temp.path().join("workspace");
        let id = brain::workspace::WorkspaceId::parse(ID).unwrap();
        brain::workspace::WorkspaceManifest::new(id)
            .write_new(&root)
            .unwrap();
        let machine = home.join(".config/brain");
        std::fs::create_dir_all(machine.join("migrations")).unwrap();
        std::fs::write(machine.join("migrations/version"), version).unwrap();
        write_json(
            &machine.join("env.json"),
            &json!({
                "schema_version": 4, "default_workspace": "alpha", "workspaces": {
                    "alpha": {"workspace_id": ID, "root": root, "local_user_id": "member", "env": {}}
                }
            }),
        );
        Self { temp, home, root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env_remove("BRAIN_WORKSPACE")
            .env_remove("BRAIN_WORKSPACE_ID")
            .output()
            .unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.home.join(format!(".cache/brain/workspaces/{ID}"))
    }

    fn startup(&self) {
        let output = self.run(&["server", "status"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn write_json(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[path = "brain_skill_migration/capabilities.rs"]
mod capabilities;
#[path = "brain_skill_migration/downgrade.rs"]
mod downgrade;
#[path = "brain_skill_migration/installer.rs"]
mod installer;
#[path = "brain_skill_migration/instructions.rs"]
mod instructions;
#[path = "brain_skill_migration/invocations.rs"]
mod invocations;
#[path = "brain_skill_migration/replacement.rs"]
mod replacement;
#[path = "brain_skill_migration/source_projection.rs"]
mod source_projection;
#[path = "brain_skill_migration/startup.rs"]
mod startup;
#[path = "brain_skill_migration/sync_arrival.rs"]
mod sync_arrival;
#[path = "brain_skill_migration/sync_fixture.rs"]
mod sync_fixture;
