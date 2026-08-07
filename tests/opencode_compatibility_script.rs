use std::{path::Path, process::Command};

#[test]
fn local_compatibility_script_runs_all_non_provider_probes_in_isolation() {
    let temporary = tempfile::tempdir().expect("temporary compatibility root");
    let log = temporary.path().join("opencode.log");
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/fake_opencode.sh");
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/check_opencode_compatibility.sh");

    let output = Command::new("/bin/sh")
        .arg(script)
        .arg(&fake)
        .env("OPENCODE_TEST_LOG", &log)
        .output()
        .expect("run local OpenCode compatibility script");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(
        stdout.contains("OpenCode 1.18.14 is compatible"),
        "{stdout}"
    );
    let log = std::fs::read_to_string(log).expect("probe log");
    for arguments in [
        "--version",
        "--help",
        "session list --help",
        "session list --format json",
        "debug config --help",
        "debug config --pure",
    ] {
        assert!(
            log.lines().any(|line| line.ends_with(arguments)),
            "missing {arguments}:\n{log}"
        );
    }
    assert_eq!(
        std::fs::read_dir(temporary.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect::<std::collections::BTreeSet<_>>(),
        std::iter::once(std::ffi::OsString::from("opencode.log")).collect(),
        "the script must clean its isolated workspace"
    );
}
