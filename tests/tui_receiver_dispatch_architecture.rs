use std::path::Path;

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
