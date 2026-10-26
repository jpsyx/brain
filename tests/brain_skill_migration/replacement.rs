//! Exercise the real installer boundary with an isolated target and a deterministic rename shim.
use super::{
    installer::{compatible, target},
    *,
};
use std::os::unix::fs::PermissionsExt as _;

fn executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Installer {
    f: Fixture,
    candidate: PathBuf,
    installed: PathBuf,
    tools: PathBuf,
    observation: PathBuf,
}

impl Installer {
    fn new() -> Self {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        f.startup();
        Self::from_fixture(f)
    }

    fn from_fixture(f: Fixture) -> Self {
        let candidate = f.temp.path().join("candidate");
        let bin = f.temp.path().join("bin");
        let tools = f.temp.path().join("tools");
        for path in [candidate.join("target/release"), bin.clone(), tools.clone()] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(
            candidate.join("Cargo.toml"),
            "[package]\nname = \"brain\"\nversion = \"0.101.0\"\n",
        )
        .unwrap();
        std::fs::copy(
            target(&f, "0.101.0"),
            candidate.join("target/release/brain"),
        )
        .unwrap();
        let installed = bin.join("brain");
        std::fs::copy(env!("CARGO_BIN_EXE_brain"), &installed).unwrap();
        executable(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
        executable(
            &tools.join("mv"),
            "#!/usr/bin/python3\nimport os, pathlib, subprocess, sys\nresult = subprocess.run([os.environ['FIXTURE_INSTALLED'], 'server', 'status'], stdout=subprocess.PIPE, stderr=subprocess.PIPE)\npathlib.Path(os.environ['FIXTURE_OBSERVATION']).write_text(str(result.returncode))\nos.execv('/bin/mv', ['/bin/mv'] + sys.argv[1:])\n",
        );
        let observation = f.temp.path().join("ordinary-status");
        Self {
            f,
            candidate,
            installed,
            tools,
            observation,
        }
    }
    fn run(&self) -> Output {
        Command::new("/bin/bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .arg("--target-source")
            .arg(&self.candidate)
            .env_clear()
            .env("HOME", &self.f.home)
            .env("XDG_CONFIG_HOME", self.f.home.join(".config"))
            .env("TMPDIR", self.f.temp.path())
            .env("BIN_DIR", self.installed.parent().unwrap())
            .env("PATH", format!("{}:/usr/bin:/bin", self.tools.display()))
            .env("FIXTURE_INSTALLED", &self.installed)
            .env("FIXTURE_OBSERVATION", &self.observation)
            .output()
            .unwrap()
    }
}

#[test]
fn receiptless_incompatible_portable_state_prevents_executable_replacement() {
    for modern in [false, true] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        let diagnostic = if modern {
            std::fs::write(
                f.root.join("AGENTS.md"),
                super::receiptless::modern_document(),
            )
            .unwrap();
            "local instruction ownership"
        } else {
            write_json(
                &f.root.join(".config/config.json"),
                &json!({"allowed_skills":["agenda"], "skills_auto_sync":false}),
            );
            "equivalent scope"
        };
        let installer = Installer::from_fixture(f);
        let before = std::fs::read(&installer.installed).unwrap();
        let agents = std::fs::read(installer.f.root.join("AGENTS.md")).ok();
        let config = std::fs::read(installer.f.root.join(".config/config.json")).ok();
        let output = installer.run();
        assert!(
            !output.status.success(),
            "receiptless downgrade was installed"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&installer.installed).unwrap(), before);
        assert_eq!(
            std::fs::read(installer.f.root.join("AGENTS.md")).ok(),
            agents
        );
        assert_eq!(
            std::fs::read(installer.f.root.join(".config/config.json")).ok(),
            config
        );
        assert!(!installer.f.root.join(".agents").exists());
        assert!(
            !installer
                .f
                .cache()
                .join("skills-install/managed.json")
                .exists()
        );
    }
}

#[test]
fn ordinary_reconciliation_between_package_apply_and_rename_cannot_leave_modern_artifacts_for_old_binary()
 {
    let installer = Installer::new();
    let result = installer.run();
    let Installer {
        f,
        installed,
        observation,
        ..
    } = installer;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !observation.exists(),
        "replacement escaped the guarded Rust coordinator"
    );
    let version = Command::new(&installed)
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    let installed_old = String::from_utf8_lossy(&version.stdout).trim() == "brain 0.101.0";
    let manifest = read_json(&f.cache().join("skills-install/managed.json"));
    assert!(installed_old);
    assert_eq!(manifest["schema_version"], 1);
}

#[test]
fn apply_failure_preserves_executable_and_retry_finishes_a_coherent_downgrade() {
    let installer = Installer::new();
    let target = installer.candidate.join("target/release/brain");
    let original = std::fs::read_to_string(&target).unwrap();
    let counter = installer.f.temp.path().join("render-count");
    let failure = format!(
        "counter = pathlib.Path({})\ncount = int(counter.read_text()) + 1 if counter.exists() else 1\ncounter.write_text(str(count))\nif count == 3: sys.exit(42)\nregistry =",
        serde_json::to_string(&counter).unwrap()
    );
    std::fs::write(&target, original.replace("registry =", &failure)).unwrap();
    let before = std::fs::read(&installer.installed).unwrap();
    let output = installer.run();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(counter).unwrap(), "3");
    assert_eq!(std::fs::read(&installer.installed).unwrap(), before);
    installer.f.startup();
    std::fs::write(&target, original).unwrap();
    let result = installer.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        read_json(&installer.f.cache().join("skills-install/managed.json"))["schema_version"],
        1
    );
}

#[test]
fn migration_stamp_failure_preserves_installed_executable_until_retry() {
    let installer = Installer::new();
    let stamp = installer.f.home.join(".config/brain/migrations/version");
    let version = std::fs::read(&stamp).unwrap();
    std::fs::remove_file(&stamp).unwrap();
    std::fs::create_dir(&stamp).unwrap();
    let before = std::fs::read(&installer.installed).unwrap();
    let output = installer.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("replace migration state"));
    assert_eq!(std::fs::read(&installer.installed).unwrap(), before);
    std::fs::remove_dir(&stamp).unwrap();
    std::fs::write(&stamp, version).unwrap();
    let output = installer.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
