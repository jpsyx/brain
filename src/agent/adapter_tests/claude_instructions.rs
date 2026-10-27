use super::*;
use crate::agent::AgentKind;

#[test]
fn oversized_unicode_append_file_reports_size_before_utf8_decoding() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("large.md"), "🙂".repeat(6000)).unwrap();
    let command = format!(
        "{}/tests/fixtures/claude/fake_claude.sh --append-system-prompt-file large.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let frontend = ClaudeFrontend::new(command, temp.path().into(), temp.path().join("projects"));
    let error = frontend
        .launch_spec(&prepared(temp.path()))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("20 KiB") && error.contains("shorten"),
        "{error}"
    );
}

fn prepared(root: &std::path::Path) -> LaunchRequest {
    let extension = root.join(".config/extensions/workspace.md");
    std::fs::create_dir_all(extension.parent().unwrap()).unwrap();
    std::fs::write(extension, "[workspace:context]\nWorkspace rule\n").unwrap();
    if !crate::workspace::WorkspaceManifest::path(root).exists() {
        crate::workspace::WorkspaceManifest::new(
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        )
        .write_new(root)
        .unwrap();
    }
    let workspace = Arc::new(
        WorkspaceContext::new(
            root,
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            WorkspaceName::parse("family").unwrap(),
            root,
            "pablo",
            root,
        )
        .unwrap(),
    );
    LaunchRequest::from_trusted_context(
        workspace,
        actor(),
        SessionPlan::resume(AgentSession::new("resumed").unwrap()),
        None,
        AccessMode::Unrestricted,
    )
    .prepare_instructions()
    .unwrap()
}

#[test]
fn claude_retains_configured_inline_and_file_append_instructions_on_resume() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("custom rules.md"),
        "Existing file rules é🙂",
    )
    .unwrap();
    for option in [
        "--append-system-prompt 'Existing inline rules'",
        "--append-system-prompt-file 'custom rules.md'",
    ] {
        let prefix = format!(
            "{}/tests/fixtures/claude/fake_claude.sh --system-prompt-snapshot on {option}",
            env!("CARGO_MANIFEST_DIR")
        );
        let frontend =
            ClaudeFrontend::new(&prefix, temp.path().into(), temp.path().join("projects"));
        let spec = frontend.launch_spec(&prepared(temp.path())).unwrap();
        let args = instructions::arguments(&spec);
        let last = args
            .windows(2)
            .rfind(|pair| pair[0] == "--append-system-prompt")
            .unwrap();
        assert!(last[1].contains("Existing"), "{}", last[1]);
        assert!(last[1].contains("Workspace rule"));
        assert!(
            args.windows(2)
                .rfind(|pair| pair[0] == "--system-prompt-snapshot")
                .is_some_and(|pair| pair[1] == "off")
        );
        assert_eq!(spec.cwd, temp.path());
        assert!(instructions::delivered(AgentKind::Claude, &spec, &args).contains("Existing"));
    }
}

#[test]
fn claude_refuses_unproven_snapshot_support_before_launch_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let frontend = ClaudeFrontend::new(
        "printf old-help",
        temp.path().into(),
        temp.path().join("projects"),
    );
    assert!(frontend.launch_spec(&prepared(temp.path())).is_err());
}

#[test]
fn claude_refuses_prefixes_that_cannot_receive_trusted_options() {
    let temp = tempfile::tempdir().unwrap();
    let command = format!(
        "{}/tests/fixtures/claude/fake_claude.sh -- user-text",
        env!("CARGO_MANIFEST_DIR")
    );
    let frontend = ClaudeFrontend::new(command, temp.path().into(), temp.path().join("projects"));
    assert!(frontend.launch_spec(&prepared(temp.path())).is_err());
}

#[test]
fn claude_composes_double_quoted_append_instructions_without_changing_literals() {
    let temp = tempfile::tempdir().unwrap();
    let command = format!(
        r#"{}/tests/fixtures/claude/fake_claude.sh --append-system-prompt "ordinary\q literal\n slash\\ quote\" dollar\$ tick\` é\🙂""#,
        env!("CARGO_MANIFEST_DIR")
    );
    let frontend = ClaudeFrontend::new(command, temp.path().into(), temp.path().join("projects"));
    let spec = frontend.launch_spec(&prepared(temp.path())).unwrap();
    let args = instructions::arguments(&spec);
    let delivered = instructions::delivered(AgentKind::Claude, &spec, &args);
    assert!(delivered.starts_with(r#"ordinary\q literal\n slash\ quote" dollar$ tick` é\🙂"#));
    assert!(delivered.contains("Workspace rule"));
}

#[test]
fn claude_probes_a_relative_executable_in_the_launch_workspace() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/fake_claude.sh"),
        temp.path().join("configured-claude"),
    )
    .unwrap();
    let frontend = ClaudeFrontend::new(
        "./configured-claude",
        temp.path().into(),
        temp.path().join("projects"),
    );
    let spec = frontend.launch_spec(&prepared(temp.path())).unwrap();
    let args = instructions::arguments(&spec);
    assert!(instructions::delivered(AgentKind::Claude, &spec, &args).contains("Workspace rule"));
}

#[test]
fn claude_help_probe_uses_the_launch_environment() {
    use std::os::unix::fs::PermissionsExt as _;
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("configured-claude");
    let fake =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/fake_claude.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$BRAIN_ACTOR_ID\" = pablo ] || exit 64\nexec {} \"$@\"\n",
            shell_quote(&fake.display().to_string()),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let frontend = ClaudeFrontend::new(
        shell_quote(&script.display().to_string()),
        temp.path().into(),
        temp.path().join("projects"),
    );
    frontend.launch_spec(&prepared(temp.path())).unwrap();
}

fn assert_composed_append_option(root: &std::path::Path, option: &str, prior: &str) {
    let base = format!(
        "KEEP='literal value' {}/tests/fixtures/claude/fake_claude.sh --model 'configured-model'",
        env!("CARGO_MANIFEST_DIR")
    );
    let suffix = "--permission-mode 'default'";
    let frontend = ClaudeFrontend::new(
        format!("{base} {option} {suffix}"),
        root.into(),
        root.join("projects"),
    );
    let spec = frontend.launch_spec(&prepared(root)).unwrap();
    let args = instructions::arguments(&spec);
    let payload = instructions::delivered(AgentKind::Claude, &spec, &args);
    assert!(
        payload.starts_with(&format!("{prior}\n\n")),
        "{option}: {payload}"
    );
    assert!(payload.contains("Workspace rule"));
    assert_eq!(
        args.iter()
            .filter(|argument| argument.as_str() == "--append-system-prompt")
            .count(),
        1
    );
    assert!(
        !args
            .iter()
            .any(|argument| argument.starts_with("--append-system-prompt-file"))
    );
    assert!(
        spec.command
            .starts_with(&format!("{base}  {suffix} --system-prompt-snapshot off")),
        "unrelated prefix bytes changed: {}",
        spec.command
    );
}

#[test]
fn claude_preserves_split_quoted_and_escaped_inline_append_options() {
    let temp = tempfile::tempdir().unwrap();
    let prior = "Existing inline rules = '\" é🙂 \\q";
    for flag in [
        "--append-system-'prompt'",
        r"--append-system-\prompt",
        "--append-system-\"prompt\"",
    ] {
        for separator in [" ", "="] {
            assert_composed_append_option(
                temp.path(),
                &format!("{flag}{separator}{}", shell_quote(prior)),
                prior,
            );
        }
    }
}

#[test]
fn claude_preserves_split_quoted_and_escaped_file_append_options() {
    let temp = tempfile::tempdir().unwrap();
    let filename = "custom rules é.md";
    let prior = "Existing file rules = '\" é🙂 \\q";
    std::fs::write(temp.path().join(filename), prior).unwrap();
    for flag in [
        "--append-system-'prompt-file'",
        r"--append-system-\prompt-file",
        "--append-system-\"prompt-file\"",
    ] {
        for separator in [" ", "="] {
            assert_composed_append_option(
                temp.path(),
                &format!("{flag}{separator}{}", shell_quote(filename)),
                prior,
            );
        }
    }
}

#[test]
fn claude_validates_files_named_by_split_quoted_and_escaped_append_options() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("invalid rules.md"), [0xff]).unwrap();
    for filename in ["missing rules.md", "invalid rules.md"] {
        for flag in [
            "--append-system-'prompt-file'",
            r"--append-system-\prompt-file",
        ] {
            for separator in [" ", "="] {
                let command = format!(
                    "{}/tests/fixtures/claude/fake_claude.sh {flag}{separator}{}",
                    env!("CARGO_MANIFEST_DIR"),
                    shell_quote(filename)
                );
                let frontend =
                    ClaudeFrontend::new(command, temp.path().into(), temp.path().join("projects"));
                let error = frontend
                    .launch_spec(&prepared(temp.path()))
                    .expect_err("configured file must be read and validated");
                assert!(
                    error
                        .to_string()
                        .contains("cannot preserve Claude launch instructions")
                );
            }
        }
    }
}
