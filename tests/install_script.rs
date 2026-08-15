use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write executable");
    let mut permissions = std::fs::metadata(path)
        .expect("executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("executable permissions");
}

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    tools: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new(installed_version: Option<&str>, target_version: &str) -> Self {
        let temporary = tempfile::tempdir().expect("temporary root");
        let root = temporary.path().join("source");
        let bin = temporary.path().join("bin");
        let tools = temporary.path().join("tools");
        let log = temporary.path().join("migrations.log");
        std::fs::create_dir_all(root.join("target/release")).expect("release directory");
        std::fs::create_dir_all(&bin).expect("bin directory");
        std::fs::create_dir_all(&tools).expect("tools directory");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"),
            root.join("install.sh"),
        )
        .expect("copy installer");
        let target = format!(
            "#!/bin/sh\nif [ \"${{1:-}}\" = --version ]; then echo 'brain {target_version}'; exit 0; fi\necho \"target $*\" >> \"$MIGRATION_LOG\"\n"
        );
        executable(&root.join("target/release/brain"), &target);
        if let Some(version) = installed_version {
            let installed = format!(
                "#!/bin/sh\nif [ \"${{1:-}}\" = --version ]; then echo 'brain {version}'; exit 0; fi\necho \"installed $*\" >> \"$MIGRATION_LOG\"\n"
            );
            executable(&bin.join("brain"), &installed);
        }
        executable(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
        Self {
            _temporary: temporary,
            root,
            bin,
            tools,
            log,
        }
    }

    fn run(&self) -> Output {
        let path = format!(
            "{}:{}",
            self.tools.display(),
            std::env::var("PATH").expect("PATH")
        );
        Command::new("bash")
            .arg(self.root.join("install.sh"))
            .env("BIN_DIR", &self.bin)
            .env("MIGRATION_LOG", &self.log)
            .env("PATH", path)
            .output()
            .expect("run installer")
    }

    fn migration_log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

#[test]
fn upgrade_runs_migrations_with_the_new_binary_after_replacement() {
    let fixture = Fixture::new(Some("0.70.0"), "0.71.0");

    let output = fixture.run();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fixture.migration_log(),
        "target __migrate --from-version 0.70.0 --to-version 0.71.0\n"
    );
}

#[test]
fn downgrade_runs_migrations_with_the_installed_binary_before_replacement() {
    let fixture = Fixture::new(Some("0.72.0"), "0.71.0");

    let output = fixture.run();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fixture.migration_log(),
        "installed __migrate --from-version 0.72.0 --to-version 0.71.0\n"
    );
}

#[test]
fn same_version_install_reconciles_with_the_replaced_binary() {
    let fixture = Fixture::new(Some("0.71.0"), "0.71.0");

    let output = fixture.run();

    assert!(output.status.success());
    assert_eq!(
        fixture.migration_log(),
        "target __migrate --from-version 0.71.0 --to-version 0.71.0\n"
    );
}

#[test]
fn fresh_install_reconciles_the_target_version() {
    let fixture = Fixture::new(None, "0.71.0");

    let output = fixture.run();

    assert!(output.status.success());
    assert_eq!(
        fixture.migration_log(),
        "target __migrate --from-version 0.71.0 --to-version 0.71.0\n"
    );
}

#[test]
fn installer_declares_the_python_runtime_prerequisite() {
    let installer =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .expect("installer source");

    assert!(installer.contains("command -v python3"));
    assert!(installer.contains("'python3' not found"));
}
