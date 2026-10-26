//! The exact current target uses the same checked projection as ordinary sync.
use super::{Fixture, assert_ok, support};
use sha2::{Digest as _, Sha256};
use std::{fs, path::Path};

#[test]
fn exact_current_target_preflight_and_apply_preserve_unmoved_customization_and_legacy_sources() {
    let mut fixture = Fixture::new();
    let root = fixture.add("alpha");
    let source = root.join(".config/extensions/todo.md");
    let mut before_source = fs::read(&source).unwrap();
    before_source.extend_from_slice(b"\n[todo:linear-backlog]\nCURRENT TASK CUSTOMIZATION\n");
    fs::write(&source, &before_source).unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_brain"));
    let digest = format!("{:x}", Sha256::digest(fs::read(executable).unwrap()));
    for phase in ["preflight", "apply"] {
        let output = fixture
            .command()
            .args(["__refresh-workspace-skills", "--target-binary"])
            .arg(executable)
            .args([
                "--target-version",
                env!("CARGO_PKG_VERSION"),
                "--phase",
                phase,
                "--expected-target-sha256",
                &digest,
            ])
            .output()
            .unwrap();
        assert_ok(&output);
        assert_eq!(fs::read(&source).unwrap(), before_source);
        if phase == "preflight" {
            assert!(!root.join(".agents").exists());
        }
    }
    let rendered = fs::read_to_string(support::todo(&root)).unwrap();
    assert_eq!(rendered.matches("CURRENT TASK CUSTOMIZATION").count(), 1);
    assert!(rendered.contains("read its `AGENTS.md`"));
    assert!(rendered.contains("brain tasks remove T17 --dry-run --json"));
    assert_eq!(fs::read(&source).unwrap(), before_source);
}
