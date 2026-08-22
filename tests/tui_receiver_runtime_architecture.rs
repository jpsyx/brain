use std::path::Path;

#[test]
fn app_owns_one_receiver_runtime_instead_of_receiver_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = std::fs::read_to_string(root.join("src/tui/mod.rs")).expect("read App source");
    let services = std::fs::read_to_string(root.join("src/tui/state/services.rs"))
        .expect("read AppServices source");

    assert!(
        app.contains("receiver: crate::tui::receiver::ReceiverRuntime"),
        "App must own one ReceiverRuntime"
    );
    assert!(
        app.contains("services: AppServices"),
        "App must compose one AppServices owner"
    );
    assert!(
        services.contains("receiver_sync_runtime: Box<dyn ReceiverSyncRuntime>"),
        "AppServices must own receiver sync effects"
    );
    for forbidden in [
        "receiver_control:",
        "receiver_enabled:",
        "receiver_intent_refresher:",
        "receiver_queue:",
        "receiver_new_session:",
        "receiver_force_fresh:",
        "requested_receiver_actor:",
        "receiver_lease:",
        "receiver_generation:",
        "receiver_sender:",
        "receiver_recipients:",
        "receiver_response_email:",
        "receiver_email_reply:",
        "receiver_session_id:",
        "interactive_session_id:",
        "interactive_agent_session_id:",
        "receiver_resume_session:",
        "receiver_started:",
        "receiver_delay_sent:",
        "receiver_probe:",
        "receiver_panel_activity:",
        "receiver_panel_sampled_at:",
        "receiver_retry_at:",
        "receiver_sync_gate:",
    ] {
        assert!(!app.contains(forbidden), "App still owns {forbidden}");
    }
    for forbidden_type in [
        "Option<crate::tui::singleton::JobSocket>",
        "Box<dyn crate::command::server::ReceiverIntentRefresher>",
        "crate::tui::receiver::InboundQueue",
        "std::collections::HashSet<crate::server::receiver::Channel>",
        "Option<receiver_state::Lease>",
        "Option<ReceiverSyncGate>",
    ] {
        assert!(
            !app.contains(forbidden_type),
            "App still owns raw receiver type {forbidden_type}"
        );
    }
}

#[test]
fn receiver_runtime_contains_no_sync_effect_adapters_or_io() {
    let receiver_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/receiver");
    let mut sources = vec![receiver_root.join("runtime.rs")];
    sources.extend(
        walkdir::WalkDir::new(receiver_root.join("runtime"))
            .into_iter()
            .map(|entry| entry.expect("walk receiver runtime source"))
            .filter(|entry| entry.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("rs")),
    );

    for path in sources {
        let source = std::fs::read_to_string(&path).expect("read receiver runtime source");
        for forbidden in [
            "ReceiverSyncRuntime",
            "SystemReceiverSyncRuntime",
            "WorkspacePaths",
            "WorkspaceContext",
            "Journal::open",
            "read_state(",
            "spawn_detached_sync",
            "std::fs",
            "std::process",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains forbidden sync effect token {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn receiver_runtime_representation_stays_in_its_module() {
    let tui_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let receiver_root = tui_root.join("receiver");
    let mut leaks = Vec::new();

    for entry in walkdir::WalkDir::new(&tui_root) {
        let entry = entry.expect("walk TUI source");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path.starts_with(&receiver_root)
        {
            continue;
        }
        let source = std::fs::read_to_string(path).expect("read TUI source");
        for field in [
            "socket",
            "enabled",
            "intent_refresher",
            "queue",
            "new_session_channels",
            "force_fresh",
            "requested_actor",
            "lease",
            "generation",
            "sender",
            "recipients",
            "response_email",
            "email_reply",
            "receiver_response_id",
            "interactive_response_id",
            "interactive_agent_session_id",
            "resume_session",
            "started",
            "delay_sent",
            "probe",
            "panel_activity",
            "panel_sampled_at",
            "retry_at",
            "sync_runtime",
            "sync_gate",
        ] {
            if directly_accesses_receiver_field(&source, field) {
                leaks.push(format!("{}: receiver.{field}", path.display()));
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "receiver runtime representation leaked outside src/tui/receiver/:\n{}",
        leaks.join("\n")
    );
}

fn directly_accesses_receiver_field(source: &str, field: &str) -> bool {
    let compact: String = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let needle = format!(".receiver.{field}");
    compact.match_indices(&needle).any(|(at, _)| {
        compact[at + needle.len()..]
            .chars()
            .next()
            .is_some_and(|next| next != '(' && !next.is_ascii_alphanumeric() && next != '_')
    })
}
