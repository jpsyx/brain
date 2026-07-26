use std::process::Command;

#[test]
fn verbose_non_tui_commands_mirror_logs_and_print_the_log_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["--verbose", "config", "get", "__missing_verbose_test_key__"])
        .output()
        .expect("run brain binary");

    assert!(
        output.status.success(),
        "brain --verbose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("verbose output is utf-8");
    assert!(stdout.contains("brain start 0.3.0"), "{stdout}");
    assert!(stdout.contains("dispatch config"), "{stdout}");

    let log_path = stdout
        .lines()
        .find_map(|line| line.strip_prefix("verbose log: "))
        .expect("verbose log path is printed");
    assert!(log_path.starts_with("/tmp/"), "{log_path}");
    assert!(
        std::path::Path::new(log_path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log")),
        "{log_path}"
    );

    let file = std::fs::read_to_string(log_path).expect("log file exists");
    assert!(file.contains("brain start 0.3.0"), "{file}");
    assert!(file.contains("dispatch config"), "{file}");
    let _ = std::fs::remove_file(log_path);
}
