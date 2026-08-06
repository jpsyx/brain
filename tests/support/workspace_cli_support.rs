use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use brain::workspace::{MachineRegistry, RegistryStore, WorkspaceManifest, WorkspaceName};
use tempfile::TempDir;

struct Fixture {
    home: TempDir,
    config_home: TempDir,
    current_dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("isolated HOME"),
            config_home: tempfile::tempdir().expect("isolated XDG_CONFIG_HOME"),
            current_dir: tempfile::tempdir().expect("isolated current directory"),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
        command
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .current_dir(self.current_dir.path());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run brain")
    }

    #[cfg(unix)]
    fn barrier_command(&self, release: &Path, args: &[&str]) -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("while [ ! -e \"$1\" ]; do :; done; shift; exec \"$@\"")
            .arg("brain-workspace-test")
            .arg(release)
            .arg(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .current_dir(self.current_dir.path());
        command
    }

    fn registry_path(&self) -> PathBuf {
        self.config_home.path().join("brain/env.json")
    }

    fn registry(&self) -> MachineRegistry {
        RegistryStore::load_from(&self.registry_path()).expect("valid isolated registry")
    }

    fn make_ready(&self, workspace: &str) {
        assert_success(&self.run(&[
            "workspace",
            "repair",
            "--local-user-id",
            "test-user",
            "-b",
            workspace,
        ]));
    }
}

fn name(value: &str) -> WorkspaceName {
    WorkspaceName::parse(value).expect("valid fixture name")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure_contains(output: &Output, expected: &[&str]) {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains('\x1b'),
        "NO_COLOR must suppress ANSI: {stderr:?}"
    );
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "missing {fragment:?} in {stderr:?}"
        );
    }
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

#[cfg(unix)]
fn fake_markdown_to_pdf(path: &Path) {
    std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("fake markdown-to-pdf");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .expect("make fake markdown-to-pdf executable");
}

#[cfg(unix)]
struct ReadOnlyDir {
    path: PathBuf,
    original_mode: u32,
}

#[cfg(unix)]
impl ReadOnlyDir {
    fn new(path: &Path) -> Self {
        let original_mode = std::fs::metadata(path)
            .expect("directory metadata")
            .permissions()
            .mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500))
            .expect("make registry directory read-only");
        Self {
            path: path.to_path_buf(),
            original_mode,
        }
    }
}

#[cfg(unix)]
impl Drop for ReadOnlyDir {
    fn drop(&mut self) {
        std::fs::set_permissions(
            &self.path,
            std::fs::Permissions::from_mode(self.original_mode),
        )
        .expect("restore registry directory permissions");
    }
}
