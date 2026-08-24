use std::path::Path;

#[test]
fn production_receiver_dispatch_uses_only_isolated_durable_runs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let facade = std::fs::read_to_string(root.join("src/tui/app_brain/receiver/mod.rs"))
        .expect("read receiver facade");
    for legacy in [
        "completion",
        "control",
        "diagnostics",
        "email_reply",
        "state",
    ] {
        let declaration = format!("mod {legacy};");
        assert!(
            !facade.contains(&declaration)
                || facade.contains(&format!("#[cfg(test)]\n{declaration}")),
            "legacy main-panel receiver module `{legacy}` must not compile in production"
        );
    }
    for current in ["active.rs", "artifact.rs", "dispatch.rs", "reply.rs"] {
        let source = std::fs::read_to_string(root.join("src/tui/app_brain/receiver").join(current))
            .unwrap_or_else(|error| panic!("read {current}: {error}"));
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
