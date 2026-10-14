use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
struct Fixture {
    temp: tempfile::TempDir,
    script: PathBuf,
    source: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}
fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn binary(path: &Path, version: &str, identity: &str) {
    let script = include_str!("target.py")
        .replace("@VERSION@", version)
        .replace("@IDENTITY@", identity);
    executable(path, &script);
}
impl Fixture {
    fn new(installed: Option<&str>, version: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("modern source");
        let source = temp.path().join("target source");
        let bin = temp.path().join("bin with spaces");
        let log = temp.path().join("protocol.jsonl");
        for root in [&script, &source] {
            fs::create_dir_all(root.join("target/release")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"brain\"\nversion = \"0.1.0\"\n",
            )
            .unwrap();
        }
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(temp.path().join("tools")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"),
            script.join("install.sh"),
        )
        .unwrap();
        binary(
            &script.join("target/release/brain"),
            env!("CARGO_PKG_VERSION"),
            "modern-source",
        );
        binary(
            &source.join("target/release/brain"),
            version,
            "selected-target",
        );
        executable(&source.join("install.sh"), "#!/bin/sh\nexit 98\n");
        executable(&source.join("run.sh"), "#!/bin/sh\nexit 99\n");
        if let Some(version) = installed {
            binary(&bin.join("brain-custom"), version, "installed-helper");
        }
        executable(
            &temp.path().join("tools/cargo"),
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" > \"$CARGO_LOG\"\nprintf '%s\\n' \"$@\" >> \"$CARGO_LOG\"\n",
        );
        Self {
            temp,
            script,
            source,
            bin,
            log,
        }
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new("/bin/bash");
        cmd.arg(self.script.join("install.sh"))
            .args(["--name", "brain-custom", "--target-source"])
            .arg(&self.source)
            .env("BIN_DIR", &self.bin)
            .env("MIGRATION_LOG", &self.log)
            .env("CARGO_LOG", self.temp.path().join("cargo.log"))
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.temp.path().join("tools").display(),
                    std::env::var("PATH").unwrap()
                ),
            );
        cmd
    }
    fn run(&self) -> Output {
        self.command().output().unwrap()
    }
    fn entries(&self) -> Vec<Value> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn clean(&self) {
        assert_eq!(fs::read_dir(&self.bin).unwrap().count(), 1);
    }
}
#[test]
fn modern_source_selector_retains_helper_and_installs_frozen_target_with_exact_old_argv() {
    let f = Fixture::new(Some(env!("CARGO_PKG_VERSION")), "0.101.0");
    let output = f.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries = f.entries();
    assert_eq!(entries.len(), 4, "{entries:?}");
    assert_eq!(entries[0]["phase"], "preflight");
    assert_eq!(entries[1]["args"][0], "__migrate");
    assert_eq!(entries[1]["args"][5], "--installer-refresh-protocol");
    assert_eq!(entries[2]["phase"], "apply");
    assert_eq!(entries[0]["identity"], "installed-helper");
    assert_eq!(entries[2]["identity"], "installed-helper");
    assert_eq!(entries[0]["digest"], entries[2]["digest"]);
    assert_ne!(entries[0]["target"], entries[2]["target"]);
    assert_eq!(
        entries[2]["target"],
        f.bin.join("brain-custom").to_str().unwrap()
    );
    assert_eq!(entries[3]["identity"], "selected-target");
    assert_eq!(
        entries[3]["args"],
        serde_json::json!(["-w", "canonical", "skills", "sync"])
    );
    assert_eq!(
        entries[3]["executable"],
        f.bin.join("brain-custom").to_str().unwrap()
    );
    assert!(
        fs::read_to_string(f.temp.path().join("cargo.log"))
            .unwrap()
            .starts_with(f.source.to_str().unwrap())
    );
    f.clean();
}
#[test]
fn upgrade_same_version_and_fresh_install_use_new_coordinator() {
    for installed in [Some("0.101.0"), Some(env!("CARGO_PKG_VERSION")), None] {
        let f = Fixture::new(installed, env!("CARGO_PKG_VERSION"));
        let output = f.run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let entries = f.entries();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0]["phase"], "preflight");
        assert_eq!(entries[0]["identity"], "selected-target");
        assert_eq!(entries[1]["args"][0], "__migrate");
        assert_eq!(entries[2]["phase"], "apply");
        f.clean();
    }
}
#[test]
fn rejected_phases_clean_helper_and_do_not_claim_success() {
    for phase in ["preflight", "down", "apply"] {
        let f = Fixture::new(Some(env!("CARGO_PKG_VERSION")), "0.101.0");
        let before = fs::read(f.bin.join("brain-custom")).unwrap();
        let output = f.command().env("FAIL_PHASE", phase).output().unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("installed brain-custom"));
        if phase == "apply" {
            assert_ne!(fs::read(f.bin.join("brain-custom")).unwrap(), before);
        } else {
            assert_eq!(fs::read(f.bin.join("brain-custom")).unwrap(), before);
        }
        f.clean();
    }
}
#[test]
fn build_artifact_change_does_not_replace_frozen_candidate() {
    let f = Fixture::new(Some(env!("CARGO_PKG_VERSION")), "0.101.0");
    let before = fs::read(f.source.join("target/release/brain")).unwrap();
    let output = f
        .command()
        .env("MUTATE_BUILD", f.source.join("target/release/brain"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(f.bin.join("brain-custom")).unwrap(), before);
    f.clean();
}
#[test]
fn historical_script_is_rejected_before_replacement_by_actual_new_binary() {
    let f = Fixture::new(Some(env!("CARGO_PKG_VERSION")), "0.101.0");
    fs::copy(env!("CARGO_BIN_EXE_brain"), f.bin.join("brain-custom")).unwrap();
    fs::copy(
        f.source.join("target/release/brain"),
        f.script.join("target/release/brain"),
    )
    .unwrap();
    fs::write(
        f.script.join("install.sh"),
        include_str!("historical-install.sh"),
    )
    .unwrap();
    let before = fs::read(f.bin.join("brain-custom")).unwrap();
    let output = Command::new("/bin/bash")
        .arg(f.script.join("install.sh"))
        .args(["--name", "brain-custom"])
        .env("BIN_DIR", &f.bin)
        .env("HOME", f.temp.path().join("home"))
        .env("XDG_CONFIG_HOME", f.temp.path().join("config"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                f.temp.path().join("tools").display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("CARGO_LOG", f.temp.path().join("cargo.log"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--target-source"));
    assert_eq!(fs::read(f.bin.join("brain-custom")).unwrap(), before);
    assert!(!f.temp.path().join("config").exists());
    f.clean();
}

#[test]
fn invalid_source_and_legacy_without_capable_helper_are_rejected() {
    for invalid in ["missing", "wrong-package", "legacy-without-helper"] {
        let f = Fixture::new(
            None,
            if invalid == "legacy-without-helper" {
                "0.101.0"
            } else {
                env!("CARGO_PKG_VERSION")
            },
        );
        if invalid == "missing" {
            fs::remove_dir_all(&f.source).unwrap();
        }
        if invalid == "wrong-package" {
            fs::write(f.source.join("Cargo.toml"), "[package]\nname = \"other\"\n").unwrap();
        }
        let output = f.run();
        assert!(!output.status.success());
        assert_eq!(fs::read_dir(&f.bin).unwrap().count(), 0);
        assert!(!f.log.exists());
        if invalid != "legacy-without-helper" {
            assert!(!f.temp.path().join("cargo.log").exists());
        }
    }
}
#[test]
fn changed_frozen_candidate_and_failed_rename_cleanup_without_success() {
    for fault in ["candidate", "rename"] {
        let f = Fixture::new(Some(env!("CARGO_PKG_VERSION")), "0.101.0");
        let before = fs::read(f.bin.join("brain-custom")).unwrap();
        let mut cmd = f.command();
        if fault == "candidate" {
            cmd.env("MUTATE_CANDIDATE", "1");
        } else {
            executable(&f.temp.path().join("tools/mv"), "#!/bin/sh\nexit 1\n");
        }
        let output = cmd.output().unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("installed brain-custom"));
        assert!(
            fs::read(f.bin.join("brain-custom")).unwrap() == before,
            "fault={fault}; stdout={}; stderr={}; protocol={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            f.entries()
        );
        f.clean();
    }
}
