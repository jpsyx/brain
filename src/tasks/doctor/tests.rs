use super::*;

#[test]
fn sync_line_reflects_rclone_and_config() {
    let plain = crate::theme::Theme::dark(false);
    assert!(sync_line(Some("1.74.2"), true, plain).contains("rclone ✓ 1.74.2"));
    assert!(sync_line(Some("1.74.2"), true, plain).contains("sync configured"));
    assert!(sync_line(None, false, plain).contains("not installed"));
    assert!(sync_line(None, false, plain).contains("sync off"));
}

#[test]
fn sync_line_colors_rclone_and_config_status() {
    let colored = crate::theme::Theme::dark(true);
    let installed = sync_line(Some("1.74.2"), true, colored);
    assert!(
        installed.contains("\x1b[92m"),
        "rclone ✓ and 'sync configured' should be success green: {installed}"
    );

    let missing = sync_line(None, false, colored);
    assert!(
        missing.contains("\x1b[91m"),
        "rclone ✗ should be error red: {missing}"
    );
    assert!(
        missing.contains("\x1b[90m"),
        "'sync off' should be muted gray: {missing}"
    );
}

#[test]
fn doctor_plan_names_every_check_before_running() {
    let plan = format_doctor_plan(
        Path::new("/tmp/brain/state.db"),
        Path::new("/tmp/brain/.claude/settings.json"),
        crate::theme::Theme::dark(false),
    );

    assert!(plan.contains("Checking brain task environment"), "{plan}");
    assert!(plan.contains("state DB: /tmp/brain/state.db"), "{plan}");
    assert!(
        plan.contains("SessionStart hook: /tmp/brain/.claude/settings.json"),
        "{plan}"
    );
    assert!(
        plan.contains("Claude: probing configured command"),
        "{plan}"
    );
    assert!(
        plan.contains("OpenCode: probing configured command"),
        "{plan}"
    );
    assert!(plan.contains("rclone: probing PATH"), "{plan}");
    assert!(plan.contains("sync config: reading brain env"), "{plan}");
}

#[test]
fn doctor_renders_read_only_opencode_compatibility_health() {
    let workspace = crate::workspace::WorkspaceName::parse("work").unwrap();
    let mut compatible = Diagnosis::default();
    compatible.record_frontend_compatibility(
        crate::agent::AgentKind::OpenCode,
        Ok(Some("1.18.14".to_owned())),
    );
    let ready = format_workspace_report(
        &compatible,
        &workspace,
        Path::new("/tmp/brain"),
        &[],
        crate::theme::Theme::dark(false),
    );
    assert!(
        ready.contains("OpenCode compatibility: ready (1.18.14)"),
        "{ready}"
    );

    let mut incompatible = Diagnosis::default();
    incompatible.record_frontend_compatibility(
        crate::agent::AgentKind::OpenCode,
        Err(crate::agent::AgentError::Frontend(
            "missing --session".to_owned(),
        )),
    );
    assert!(!incompatible.is_ok());
    let broken = format_workspace_report(
        &incompatible,
        &workspace,
        Path::new("/tmp/brain"),
        &[],
        crate::theme::Theme::dark(false),
    );
    assert!(broken.contains("missing --session"), "{broken}");
}

#[test]
fn doctor_remediation_uses_exact_noninteractive_paths() {
    let workspace = crate::workspace::WorkspaceName::parse("work").unwrap();
    let output = format_workspace_report(
        &Diagnosis::default(),
        &workspace,
        Path::new("/tmp/brain root"),
        &[],
        crate::theme::Theme::dark(false),
    );

    assert!(
        output.contains("scripts/install_hook.sh' '/tmp/brain root'"),
        "{output}"
    );
    assert!(!output.contains("<WORKSPACE_ROOT>"), "{output}");
}
