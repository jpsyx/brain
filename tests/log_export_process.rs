use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

fn command(home: &Path, bin: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
    command
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("NO_COLOR", "1")
        .env("EXPORT_ARGS", home.join("opened"))
        .env("EXPORT_COPY", home.join("clipboard"))
        .current_dir(home);
    command
}

fn executable(path: &Path, source: &str) {
    std::fs::write(path, source).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn cli_opens_the_actual_file_as_text_and_copies_diagnostic_content() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    executable(
        &bin.join("open"),
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$EXPORT_ARGS\"\necho native-opener-output\n",
    );
    executable(
        &bin.join("pbcopy"),
        "#!/bin/sh\n/bin/cat > \"$EXPORT_COPY\"\necho native-clipboard-output\necho native-clipboard-diagnostic >&2\nif [ \"$EXPORT_EXIT\" = 7 ]; then exit 7; fi\nexit 0\n",
    );
    let log = home.join("run with spaces.log");
    std::fs::write(
        &log,
        "context before failure\nreceive_pending [receive]: full error\n",
    )
    .unwrap();
    let output = command(home, &bin).arg("logs").arg(&log).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(home.join("opened")).unwrap(),
        format!("-t\n{}\n", log.display())
    );
    let output = command(home, &bin)
        .arg("logs")
        .arg(&log)
        .arg("--copy")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let copied = std::fs::read_to_string(home.join("clipboard")).unwrap();
    assert!(copied.contains("context before failure\nreceive_pending [receive]: full error"));
    assert!(copied.contains(&log.display().to_string()));
    let output = command(home, &bin)
        .env("EXPORT_EXIT", "7")
        .arg("logs")
        .arg(&log)
        .arg("--copy")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Could not export"), "{error}");
    assert!(!error.contains("✓"));
    assert!(!error.contains("native-clipboard-diagnostic"));
    let output = command(home, &bin)
        .env("EXPORT_EXIT", "7")
        .args(["--verbose", "logs"])
        .arg(&log)
        .arg("--copy")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("native-clipboard-diagnostic"));
}

#[test]
fn logs_help_does_not_create_workspace_or_runtime_state() {
    let temp = tempfile::tempdir().unwrap();
    let output = command(temp.path(), temp.path())
        .args(["logs", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--copy"));
    assert!(help.contains("Examples:"));
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}
