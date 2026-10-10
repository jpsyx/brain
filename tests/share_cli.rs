use std::path::Path;
use std::process::Command;

fn command(home: &Path, root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("NO_COLOR", "1")
        .env_remove("BRAIN_WORKSPACE_ID")
        .env_remove(brain::workspace::STRICT_ENV)
        .current_dir(root);
    command
}

#[test]
fn cli_resolves_from_cwd_and_reports_path_expiration_and_configuration_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("brain");
    let created = command(&home, tmp.path())
        .args(["workspace", "create", "--name", "example", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let repaired = command(&home, &root)
        .args(["workspace", "repair", "--local-user-id", "tester"])
        .output()
        .unwrap();
    assert!(
        repaired.status.success(),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    std::fs::create_dir_all(root.join("resources/book")).unwrap();
    std::fs::write(root.join("resources/paper.pdf"), b"PDF").unwrap();
    for (args, expected) in [
        (
            vec!["share", "missing.pdf"],
            "not a valid file or directory",
        ),
        (
            vec!["share", "paper.pdf", "--expires", "1 year"],
            "at most 7 days",
        ),
        (vec!["share", "paper.pdf"], "brain sync setup"),
        (vec!["share", "book"], "brain sync setup"),
    ] {
        let output = command(&home, &root.join("resources"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!stderr.contains('\x1b'));
    }
    assert!(!root.join("resources/book/book.zip").exists());
}
