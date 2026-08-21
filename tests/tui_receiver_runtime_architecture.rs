use std::path::Path;

#[test]
fn app_owns_one_receiver_runtime_instead_of_receiver_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = std::fs::read_to_string(root.join("src/tui/mod.rs")).expect("read App source");

    assert!(
        app.contains("receiver: crate::tui::receiver::ReceiverRuntime"),
        "App must own one ReceiverRuntime"
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
        "receiver_sync_runtime:",
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
        "Box<dyn ReceiverSyncRuntime>",
        "Option<ReceiverSyncGate>",
    ] {
        assert!(
            !app.contains(forbidden_type),
            "App still owns raw receiver type {forbidden_type}"
        );
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
