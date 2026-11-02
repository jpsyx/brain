use brain::logging::export::recent_logs;
use std::fmt::Write as _;

#[test]
fn recent_logs_include_context_and_keep_the_newest_two_thousand_lines() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("run.log");
    let mut content = String::new();
    for index in 0..2100 {
        writeln!(content, "event {index}").unwrap();
    }
    std::fs::write(&path, content).unwrap();
    let exported = recent_logs(&path).unwrap();
    assert!(exported.contains("Brain version:"));
    assert!(exported.contains(&path.display().to_string()));
    assert!(exported.contains("Older log content omitted"));
    assert!(!exported.contains("event 99\n"));
    assert!(exported.contains("event 100\n"));
    assert!(exported.ends_with("event 2099\n"));
}

#[test]
fn recent_logs_bound_large_unicode_lines_and_keep_the_latest_failure() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("run.log");
    std::fs::write(
        &path,
        format!("{}\nlaunch failed: full diagnostic\n", "界".repeat(100_000)),
    )
    .unwrap();
    let exported = recent_logs(&path).unwrap();
    assert!(exported.len() < 263_000);
    assert!(exported.contains("Older log content omitted"));
    assert!(exported.ends_with("launch failed: full diagnostic\n"));
    assert!(!exported.contains('\u{fffd}'));
}

#[test]
fn recent_logs_do_not_hide_io_failures_or_claim_empty_logs_are_useful() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("run.log");
    assert!(recent_logs(&path).is_err());
    std::fs::write(&path, "").unwrap();
    assert!(recent_logs(&path).is_err());
}

#[test]
fn cli_exposes_open_and_copy_without_needing_a_workspace_target() {
    use brain::cli::{Cmd, try_parse_from};
    use brain::workspace::{BootstrapPolicy, bootstrap_policy, invocation_for};
    for args in [
        vec!["brain", "logs", "/tmp/run.log"],
        vec!["brain", "logs", "/tmp/run.log", "--copy"],
    ] {
        let parsed = try_parse_from(args).unwrap();
        assert_eq!(
            bootstrap_policy(invocation_for(&parsed)),
            BootstrapPolicy::None
        );
        assert!(matches!(parsed.command, Some(Cmd::Logs(_))));
    }
}
