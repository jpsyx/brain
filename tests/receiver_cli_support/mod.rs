//! A two-workspace machine for the informational `brain receiver` surfaces.
//!
//! Drives the compiled binary with an isolated `HOME`/`XDG_CONFIG_HOME` and
//! never starts a server, which is the point: these commands answer before
//! ingress is ever live.

use std::path::PathBuf;
use std::process::{Command, Output};

use tempfile::TempDir;

pub struct Machine {
    home: TempDir,
    config_home: TempDir,
}

impl Machine {
    /// Two registered workspaces (`brain` is default, `family` is not), each
    /// with a portable local user so ordinary commands are ready.
    pub fn new() -> Self {
        let machine = Self {
            home: tempfile::tempdir().expect("home tempdir"),
            config_home: tempfile::tempdir().expect("config tempdir"),
        };
        for workspace in ["brain", "family"] {
            let root = machine.home.path().join(workspace);
            machine.ok(&[
                "workspace",
                "create",
                "--name",
                workspace,
                "--root",
                root.to_str().expect("root path"),
            ]);
            machine.ok(&[
                "user", "add", "-w", workspace, "--id", "pablo", "--name", "Pablo",
            ]);
            machine.ok(&["user", "local", "pablo", "-w", workspace]);
        }
        machine
    }

    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run brain {args:?}: {error}"))
    }

    pub fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "brain {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }

    /// The stable ingress UUID from a workspace's portable manifest.
    pub fn ingress(&self, workspace: &str) -> String {
        let manifest = self
            .home
            .path()
            .join(workspace)
            .join(".config/workspace.json");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).expect("read manifest"))
                .expect("manifest JSON");
        manifest["receiver_ingress_id"]
            .as_str()
            .expect("ingress id")
            .to_owned()
    }

    #[allow(dead_code)]
    pub fn workspace_id(&self, workspace: &str) -> brain::workspace::WorkspaceId {
        let registry: serde_json::Value = serde_json::from_slice(
            &std::fs::read(self.config_home.path().join("brain/env.json"))
                .expect("read machine registry"),
        )
        .expect("registry JSON");
        brain::workspace::WorkspaceId::parse(
            registry["workspaces"][workspace]["workspace_id"]
                .as_str()
                .expect("workspace ID"),
        )
        .expect("valid workspace ID")
    }

    #[allow(dead_code)]
    pub fn state_db(&self, workspace: &str) -> PathBuf {
        brain::workspace::WorkspacePaths::new(self.home.path(), self.workspace_id(workspace))
            .state_db()
    }
}
