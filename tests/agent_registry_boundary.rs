#[test]
fn transcript_lookup_is_not_part_of_the_public_agent_facade() {
    let controller = include_str!("../src/agent/controller/mod.rs");
    let frontend = include_str!("../src/agent/frontend.rs");

    assert!(!controller.contains("pub fn transcript"));
    assert!(!frontend.contains("fn transcript("));
}

#[test]
fn production_callers_do_not_name_concrete_frontend_types() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in walkdir::WalkDir::new(&source_root) {
        let entry = entry.expect("walk production source");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path.starts_with(source_root.join("agent"))
        {
            continue;
        }
        let source = std::fs::read_to_string(path).expect("read production source");
        for concrete in ["ClaudeFrontend", "CodexFrontend", "OpenCodeFrontend"] {
            assert!(
                !source.contains(concrete),
                "{} names {concrete}",
                path.display()
            );
        }
    }
}

#[test]
fn concrete_frontends_and_adapter_operations_are_not_publicly_exported() {
    let module = include_str!("../src/agent/mod.rs");
    let frontend = include_str!("../src/agent/frontend.rs");

    for public_adapter_surface in [
        "pub mod frontend",
        "pub mod input",
        "pub use claude::ClaudeFrontend",
        "pub use codex::CodexFrontend",
        "pub use opencode::OpenCodeFrontend",
        "pub use frontend::{AgentAction, AgentFrontend",
    ] {
        assert!(
            !module.contains(public_adapter_surface),
            "agent facade leaks `{public_adapter_surface}`"
        );
    }

    assert!(
        !frontend.contains("pub fn new(\n        command"),
        "transport DTO exposes its adapter-side constructor"
    );
    assert!(
        frontend.contains("#[non_exhaustive]\npub struct LaunchSpec"),
        "transport DTO can be constructed directly outside the facade"
    );
}

#[test]
fn shared_installation_and_rollback_code_do_not_branch_on_frontend_artifacts() {
    let installer = include_str!("../src/command/server/receiver/hooks.rs");
    let transaction = include_str!("../src/command/server/receiver/setup/transaction.rs");

    for concrete in [
        "LifecycleInstallation::ClaudeHooks",
        "LifecycleInstallation::CodexHooks",
        "LifecycleInstallation::OpenCodePlugin",
    ] {
        assert!(
            !installer.contains(concrete),
            "installer contains {concrete}"
        );
    }
    for frontend_path in [".claude/", ".codex/", ".opencode/"] {
        assert!(
            !transaction.contains(frontend_path),
            "rollback hard-codes frontend path {frontend_path}"
        );
    }
}

fn production_prefix(source: &str) -> &str {
    source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(production, _)| production)
}

fn observation_boundary_violation(relative: &str, source: &str) -> Option<&'static str> {
    for forbidden in [
        "ClaudeFrontend",
        "CodexFrontend",
        "OpenCodeFrontend",
        "AgentKind::Claude",
        "AgentKind::Codex",
        "AgentKind::OpenCode",
        r#""claude""#,
        r#""codex""#,
        r#""opencode""#,
        r#""open-code""#,
        r#""Claude""#,
        r#""Codex""#,
        r#""OpenCode""#,
        ".claude/",
        ".codex/",
        ".opencode/",
        ".jsonl",
        "rollout-",
        "message.part.updated",
        "session.updated",
        "UserPromptSubmit",
        "PostToolUse",
        "read_normalized_snapshot",
        "RawSnapshot",
        "ParsedSnapshot",
        "snapshot_revision",
        "durable_revision",
        "accepted_at_unix_ms",
        "progressing_at_unix_ms",
        "latest_progress_at_unix_ms",
        "completed_at_unix_ms",
    ] {
        if source.contains(forbidden) {
            return Some(forbidden);
        }
    }
    if relative != "tui/app_brain/receiver/launch.rs"
        && source.contains("receiver_observations_dir")
    {
        return Some("receiver_observations_dir");
    }
    None
}

#[test]
fn receiver_observation_guard_rejects_provider_branches_literals_and_bypasses() {
    for (label, relative, mutation) in [
        (
            "provider branch",
            "tui/state/brain/ephemeral.rs",
            "match kind { AgentKind::Claude => observe() }",
        ),
        (
            "provider literal",
            "tui/state/brain/ephemeral.rs",
            r#"let provider = "codex";"#,
        ),
        (
            "concrete adapter",
            "tui/state/brain/ephemeral.rs",
            "OpenCodeFrontend::new(command)",
        ),
        (
            "concrete parser",
            "tui/state/brain/ephemeral.rs",
            "read_normalized_snapshot(request)",
        ),
        (
            "path ownership",
            "tui/state/brain/ephemeral.rs",
            "paths.receiver_observations_dir()",
        ),
        (
            "raw snapshot revision",
            "tui/state/services.rs",
            "let revision = result.snapshot_revision();",
        ),
        (
            "opaque cursor extraction",
            "tui/state/services.rs",
            "let revision = result.next_cursor().durable_revision();",
        ),
    ] {
        assert!(
            observation_boundary_violation(relative, mutation).is_some(),
            "guard accepted {label}"
        );
    }

    assert_eq!(
        observation_boundary_violation(
            "tui/state/services.rs",
            "ReceiverObservationSet::from_agent_observation(token, registration, result, now)",
        ),
        None,
        "guard rejected the neutral agent-to-state conversion seam"
    );
}

#[test]
fn receiver_observation_coordination_cannot_name_provider_or_snapshot_grammar() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = vec![
        source_root.join("tui/state/brain/ephemeral.rs"),
        source_root.join("tui/state/services.rs"),
    ];
    for root in [
        source_root.join("tui/receiver"),
        source_root.join("tui/app_brain/receiver"),
    ] {
        for entry in walkdir::WalkDir::new(&root) {
            let entry = entry.expect("walk receiver source");
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
                || path
                    .strip_prefix(&source_root)
                    .expect("source path")
                    .components()
                    .any(|component| component.as_os_str().to_string_lossy().contains("test"))
            {
                continue;
            }
            paths.push(path.to_path_buf());
        }
    }
    for path in paths {
        let relative = path
            .strip_prefix(&source_root)
            .expect("receiver source path")
            .to_string_lossy();
        let source = std::fs::read_to_string(&path).expect("read receiver source");
        assert_eq!(
            observation_boundary_violation(&relative, production_prefix(&source)),
            None,
            "{} bypasses AgentController observation ownership",
            path.display()
        );
    }
}
