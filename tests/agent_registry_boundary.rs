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

#[test]
fn receiver_observation_coordination_cannot_name_provider_or_snapshot_grammar() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for relative in ["tui/receiver", "tui/app_brain/receiver"] {
        let root = source_root.join(relative);
        for entry in walkdir::WalkDir::new(&root) {
            let entry = entry.expect("walk receiver source");
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
                || path.to_string_lossy().contains("tests")
            {
                continue;
            }
            let source = std::fs::read_to_string(path).expect("read receiver source");
            for forbidden in [
                "ClaudeFrontend",
                "CodexFrontend",
                "OpenCodeFrontend",
                ".jsonl",
                "rollout-",
                "message.part.updated",
                "session.updated",
                "UserPromptSubmit",
                "PostToolUse",
                "read_normalized_snapshot",
                "accepted_at_unix_ms",
                "progressing_at_unix_ms",
                "completed_at_unix_ms",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{} bypasses AgentController with `{forbidden}`",
                    path.display()
                );
            }
            if path.file_name().and_then(|name| name.to_str()) != Some("launch.rs") {
                assert!(
                    !source.contains("receiver_observations_dir"),
                    "{} reads an observation path outside launch/controller ownership",
                    path.display()
                );
            }
        }
    }
}
