use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn receiver_owned_job_socket_representation_is_rejected() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "use crate::tui::singleton::JobSocket as Endpoint;\npub struct Runtime { endpoint: Option<Endpoint> }\n",
        ),
    ]);

    assert_violation(fixture.path(), "receiver-owned JobSocket", "JobSocket");
}

#[test]
fn receiver_owned_warm_panel_authority_is_rejected_by_shape_through_aliases_and_wrappers() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "neutral.rs",
            "pub type ReceiverLane = crate::server::receiver::job::Channel;\npub type TurnGeneration = u64;\npub type Expiration = std::time::Instant;\npub struct PanelTurnAuthority { channel: ReceiverLane, generation: TurnGeneration, deadline: Expiration }\npub type WarmTurn = PanelTurnAuthority;\n",
        ),
        (
            "receiver.rs",
            "type HeldTurn = Option<crate::neutral::WarmTurn>;\npub struct Runtime { authority: HeldTurn }\n",
        ),
    ]);

    assert_violation(
        fixture.path(),
        "receiver-owned warm-panel authority",
        "aliased and wrapped warm-panel authority",
    );
}

#[test]
fn unrelated_receiver_local_lease_is_allowed() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod tui;\n"),
        ("tui.rs", "pub mod receiver;\n"),
        (
            "tui/receiver.rs",
            "pub struct ReceiverRetryLease { attempts: u8, next_retry: std::time::Instant }\n",
        ),
    ]);

    assert_no_violations(fixture.path(), "unrelated receiver-local lease");
}

#[test]
fn every_receiver_reachable_agent_input_and_activity_operation_is_rejected() {
    for (operation, invocation, expected) in [
        (
            "type_text",
            "controller.type_text(\"prompt\");",
            "interactive AgentController type_text",
        ),
        (
            "submit_now",
            "controller.submit_now();",
            "interactive AgentController submit_now",
        ),
        (
            "queue_after_active_turn",
            "controller.queue_after_active_turn(\"prompt\");",
            "interactive AgentController queue_after_active_turn",
        ),
        (
            "start_new_session",
            "controller.start_new_session();",
            "interactive AgentController start_new_session",
        ),
        (
            "forward_terminal_input",
            "controller.forward_terminal_input(b'p');",
            "interactive AgentController forward_terminal_input",
        ),
        (
            "snapshot",
            "let _ = controller.snapshot();",
            "interactive AgentController activity sample",
        ),
        (
            "terminal_screen",
            "let _ = controller.terminal_screen();",
            "interactive AgentController activity sample",
        ),
    ] {
        let receiver = format!(
            "pub fn dispatch(controller: &mut crate::agent::controller::AgentController) {{ {invocation} }}\n"
        );
        let fixture = rust_fixture(&[("lib.rs", "mod receiver;\n"), ("receiver.rs", &receiver)]);

        assert_violation(fixture.path(), expected, operation);
    }
}

#[test]
fn newly_introduced_receiver_reachable_agent_controller_method_fails_closed() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "pub fn dispatch(controller: &mut crate::agent::controller::AgentController) { controller.new_typed_operation(); }\n",
        ),
    ]);

    assert_violation(
        fixture.path(),
        "unclassified AgentController operation",
        "new typed AgentController method",
    );
}

#[test]
fn every_receiver_reachable_main_or_selected_panel_operation_is_rejected() {
    for (operation, invocation, expected) in [
        (
            "open_or_focus_brain",
            "app.open_or_focus_brain();",
            "interactive main-panel focus",
        ),
        (
            "take_main",
            "let _ = app.take_main();",
            "interactive main-panel controller access",
        ),
        (
            "install_main",
            "app.install_main(None);",
            "interactive main-panel controller access",
        ),
        (
            "main_controller",
            "let _ = app.main_controller();",
            "interactive main-panel controller access",
        ),
        (
            "main_controller_mut",
            "let _ = app.main_controller_mut();",
            "interactive main-panel controller access",
        ),
        (
            "active_brain_controller",
            "let _ = app.active_brain_controller();",
            "interactive selected-panel controller access",
        ),
        (
            "active_brain_controller_mut",
            "let _ = app.active_brain_controller_mut();",
            "interactive selected-panel controller access",
        ),
        (
            "focus_brain",
            "app.focus_brain();",
            "interactive selected-panel takeover",
        ),
        (
            "select_brain_tab",
            "app.select_brain_tab(0);",
            "interactive selected-panel takeover",
        ),
        (
            "select_brain_tab_slot",
            "app.select_brain_tab_slot(0);",
            "interactive selected-panel takeover",
        ),
        (
            "cycle_brain_tab",
            "app.cycle_brain_tab(true);",
            "interactive selected-panel takeover",
        ),
    ] {
        let receiver = format!("pub fn dispatch(app: &mut crate::tui::App) {{ {invocation} }}\n");
        let fixture = rust_fixture(&[("lib.rs", "mod receiver;\n"), ("receiver.rs", &receiver)]);

        assert_violation(fixture.path(), expected, operation);
    }
}

#[test]
fn receiver_reachable_direct_activity_waits_are_rejected() {
    for (operation, invocation) in [
        (
            "std::thread::sleep",
            "std::thread::sleep(std::time::Duration::from_secs(30));",
        ),
        (
            "tokio::time::sleep",
            "let _ = ::tokio::time::sleep(std::time::Duration::from_secs(30));",
        ),
        (
            "std::thread::park_timeout",
            "std::thread::park_timeout(std::time::Duration::from_secs(30));",
        ),
    ] {
        let neutral = format!("pub fn wait_for_panel_activity() {{ {invocation} }}\n");
        let fixture = rust_fixture(&[
            ("lib.rs", "mod neutral;\nmod receiver;\n"),
            (
                "receiver.rs",
                "pub fn dispatch() { crate::neutral::wait_for_panel_activity(); }\n",
            ),
            ("neutral.rs", &neutral),
        ]);

        assert_violation(fixture.path(), "blocking activity wait", operation);
    }
}

#[test]
fn receiver_reachable_method_based_activity_waits_are_rejected() {
    for operation in ["park_timeout", "wait_timeout", "wait_timeout_while"] {
        let receiver = format!(
            "pub fn dispatch(activity: &crate::panel::ActivityWait) {{ activity.{operation}(std::time::Duration::from_secs(30)); }}\n"
        );
        let fixture = rust_fixture(&[("lib.rs", "mod receiver;\n"), ("receiver.rs", &receiver)]);

        assert_violation(fixture.path(), "blocking activity wait", operation);
    }
}

#[test]
fn receiver_reachable_inbound_job_queues_and_channels_are_rejected() {
    for (operation, parameter, invocation, expected) in [
        (
            "VecDeque::pop_front",
            "jobs: &mut std::collections::VecDeque<crate::server::receiver::job::InboundJob>",
            "let _ = jobs.pop_front();",
            "in-memory receiver queue consume",
        ),
        (
            "Receiver::try_recv",
            "jobs: &std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>",
            "let _ = jobs.try_recv();",
            "in-memory receiver channel consume",
        ),
    ] {
        let receiver = format!("pub fn dispatch({parameter}) {{ {invocation} }}\n");
        let fixture = rust_fixture(&[("lib.rs", "mod receiver;\n"), ("receiver.rs", &receiver)]);

        assert_violation(fixture.path(), expected, operation);
    }
}

#[test]
fn unrelated_interactive_panels_effect_queues_and_watchdogs_remain_allowed() {
    let fixture = rust_fixture(&[
        (
            "lib.rs",
            "mod delivery;\nmod interactive;\nmod receiver;\nmod server_watchdog;\n#[cfg(test)] mod fixture;\n",
        ),
        (
            "receiver.rs",
            "pub fn schedule(cleanups: &mut std::collections::VecDeque<String>, attachment_results: &std::sync::mpsc::Receiver<String>, ready: &std::sync::Condvar) { cleanups.push_back(String::new()); let _ = attachment_results.try_recv(); let _ = ready.wait_timeout((), std::time::Duration::from_millis(1)); }\n",
        ),
        (
            "interactive.rs",
            "pub fn drive(app: &mut crate::tui::App, controller: &mut crate::agent::controller::AgentController) { let _ = app.active_brain_controller_mut(); controller.type_text(\"user input\"); controller.submit_now(); controller.queue_after_active_turn(\"follow-up\"); }\n",
        ),
        (
            "delivery.rs",
            "pub fn consume(results: &mut std::collections::VecDeque<String>) { let _ = results.pop_front(); }\n",
        ),
        (
            "server_watchdog.rs",
            "pub struct Lease;\npub fn wait(activity: &crate::server::Watchdog) { std::thread::sleep(std::time::Duration::from_millis(1)); activity.wait_timeout(std::time::Duration::from_millis(1)); }\n",
        ),
        (
            "fixture.rs",
            "pub fn consume(mut jobs: std::collections::VecDeque<crate::server::receiver::job::InboundJob>) { let _ = jobs.pop_front(); }\n",
        ),
    ]);

    assert_no_violations(
        fixture.path(),
        "valid non-receiver consumers and transient holders",
    );
}

fn assert_violation(root: &std::path::Path, expected: &str, mutation: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(expected)),
        "{mutation} mutation escaped its structural classifier branch: {violations:?}"
    );
}

fn assert_no_violations(root: &std::path::Path, control: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations.is_empty(),
        "{control} must remain available: {violations:?}"
    );
}
