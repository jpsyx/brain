use std::path::{Path, PathBuf};

fn production_sources(root: &Path) -> Vec<(PathBuf, String)> {
    walkdir::WalkDir::new(root.join("src"))
        .into_iter()
        .map(|entry| entry.expect("walk TUI source"))
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("rs"))
        .filter(|entry| {
            !entry.path().components().any(|part| {
                let part = part.as_os_str().to_string_lossy();
                part == "tests" || part.ends_with("_test_sections")
            })
        })
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy();
            !name.ends_with("_tests.rs") && name != "test_support.rs"
        })
        .map(|entry| {
            let path = entry.into_path();
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (path, source)
        })
        .collect()
}

fn receiver_execution_source(path: &Path, source: &str) -> bool {
    path.components().any(|part| part.as_os_str() == "receiver")
        || [
            "begin_session_launch",
            "begin_session_selection",
            "record_session_started",
            "record_session_launch_failed",
            "interactive_completion_to_clear",
            "remote_turn_in_flight",
            "receiver_panel_is_warm",
            "receiver_response_id",
            "panel_activity",
            "sample_panel_activity",
            "request_receiver_launch",
            "finish_dispatch",
        ]
        .into_iter()
        .any(|receiver_execution| source.contains(receiver_execution))
}

fn receiver_interactive_violations(path: &Path, source: &str) -> Vec<&'static str> {
    if !receiver_execution_source(path, source) {
        return Vec::new();
    }
    [
        "open_or_focus_brain",
        "queue_after_active_turn",
        ".type_text(",
        ".submit_now(",
        "take_main(",
        "install_main(",
        "main_controller(",
        "close_receiver_panel",
        "brain_turn_active",
        "remote_turn_in_flight",
        "receiver_panel_is_warm",
        "interactive_response_id",
        "receiver_response_id",
        "panel_activity",
        "sample_panel_activity",
        "screen_digest",
        "ActivityProbe",
        "InteractiveCompletion",
        "WarmLease",
        "request_receiver_launch",
        "finish_dispatch",
    ]
    .into_iter()
    .filter(|forbidden| source.contains(forbidden))
    .collect()
}

fn receiver_memory_consumer_violations(source: &str) -> Vec<&'static str> {
    [
        "InboundQueue",
        "StagedAdmission",
        "VecDeque<InboundJob>",
        "poll_jobs(",
        "process_job_stream(",
        "forward_job(",
        "forward_or_unavailable(",
        "forward_serialized_until",
    ]
    .into_iter()
    .filter(|forbidden| source.contains(forbidden))
    .collect()
}

#[test]
fn production_receiver_dispatch_uses_only_isolated_durable_runs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let facade = std::fs::read_to_string(root.join("src/tui/app_brain/receiver/mod.rs"))
        .expect("read receiver facade");
    for legacy in ["completion", "diagnostics", "email_reply", "state"] {
        let declaration = format!("mod {legacy};");
        assert!(
            !facade.contains(&declaration)
                || facade.contains(&format!("#[cfg(test)]\n{declaration}")),
            "legacy main-panel receiver module `{legacy}` must not compile in production"
        );
    }
    assert!(
        facade.contains("mod control;"),
        "durable receiver controls must compile with the live consumer"
    );
    let runtime_facade = std::fs::read_to_string(root.join("src/tui/receiver/mod.rs"))
        .expect("read receiver runtime facade");
    assert!(
        !runtime_facade.contains("#[allow(dead_code)]\nmod runtime;"),
        "live durable runtime APIs must not hide behind a module-wide dead-code allowance"
    );
    let runtime = std::fs::read_to_string(root.join("src/tui/receiver/runtime.rs"))
        .expect("read receiver runtime");
    for operation in [
        "take_durable_run",
        "store_durable_run",
        "is_enabled",
        "record_intent",
    ] {
        let declaration = format!("fn {operation}");
        let offset = runtime
            .find(&declaration)
            .unwrap_or_else(|| panic!("missing live durable runtime operation {operation}"));
        let attributes = runtime[..offset]
            .lines()
            .rev()
            .take_while(|line| !line.trim().is_empty())
            .take(8)
            .collect::<Vec<_>>();
        assert!(
            !attributes
                .iter()
                .any(|line| line.contains("allow(dead_code)")),
            "live durable runtime operation {operation} must remain linted"
        );
    }
    for current in [
        "active.rs",
        "artifact.rs",
        "control.rs",
        "dispatch.rs",
        "reply.rs",
    ] {
        let source = std::fs::read_to_string(root.join("src/tui/app_brain/receiver").join(current))
            .unwrap_or_else(|error| panic!("read {current}: {error}"));
        assert!(
            !source.contains("allow(dead_code)"),
            "live receiver module {current} must remain fully linted"
        );
        for forbidden in [
            "open_or_focus_brain",
            "queue_after_active_turn",
            "type_text",
            "submit_now",
            "take_main",
            "install_main",
        ] {
            assert!(
                !source.contains(forbidden),
                "{current} contains forbidden main-panel receiver call `{forbidden}`"
            );
        }
    }
    let control = std::fs::read_to_string(root.join("src/tui/app_brain/receiver/control.rs"))
        .expect("read durable receiver control");
    let dispatch = std::fs::read_to_string(root.join("src/tui/app_brain/receiver/dispatch.rs"))
        .expect("read durable receiver dispatch");
    for operation in ["apply_receiver_restarts", "complete_receiver_new_session"] {
        assert!(
            control.contains(operation),
            "missing live control {operation}"
        );
        assert!(
            dispatch.contains(operation),
            "durable dispatch does not invoke live control {operation}"
        );
    }
}

#[test]
fn event_loop_has_only_one_receiver_consumer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui = root.join("src/tui");
    let consumers = walkdir::WalkDir::new(&tui)
        .into_iter()
        .map(|entry| entry.expect("walk TUI source"))
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("rs"))
        .filter(|entry| {
            !entry
                .path()
                .components()
                .any(|part| part.as_os_str() == "tests")
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .map(|source| source.matches(".tick_receiver()").count())
        .sum::<usize>();

    assert_eq!(
        consumers, 1,
        "receiver dispatch must have one live consumer"
    );
}

#[test]
fn receiver_source_cannot_reach_interactive_execution_or_activity_waits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for (path, source) in production_sources(root) {
        for forbidden in receiver_interactive_violations(&path, &source) {
            violations.push(format!("{}: {forbidden}", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "receiver-facing production source reaches the interactive panel or waits on its activity:\n{}",
        violations.join("\n")
    );
}

#[test]
fn receiver_source_has_no_socket_or_in_memory_consumer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for (path, source) in production_sources(root) {
        for forbidden in receiver_memory_consumer_violations(&source) {
            violations.push(format!("{}: {forbidden}", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "production source retains a socket or in-memory receiver consumer:\n{}",
        violations.join("\n")
    );
}

#[test]
fn receiver_guards_cover_orphaned_and_indirect_production_source() {
    let orphan = Path::new("src/tui/receiver/orphaned_warm_panel.rs");
    assert!(
        !receiver_interactive_violations(
            orphan,
            "controller.queue_after_active_turn(prompt); sample_panel_activity(now);",
        )
        .is_empty()
    );
    assert!(
        !receiver_interactive_violations(
            Path::new("src/tui/app_brain/launch.rs"),
            "self.receiver.request_receiver_launch(actor); controller.submit_now();",
        )
        .is_empty()
    );
    assert!(
        !receiver_memory_consumer_violations(
            "fn consume(queue: &mut InboundQueue) { socket.poll_jobs(id, queue); }",
        )
        .is_empty()
    );
}
