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

    assert_violation(fixture.path(), "receiver-owned JobSocket");
}

#[test]
fn tui_receiver_owned_warm_panel_lease_is_rejected() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod tui;\n"),
        ("tui.rs", "pub mod receiver;\n"),
        (
            "tui/receiver.rs",
            "pub struct PanelLease { channel: String, generation: u64, deadline: std::time::Instant }\n",
        ),
    ]);

    assert_violation(fixture.path(), "warm-panel lease");
}

#[test]
fn receiver_reachable_blocking_activity_wait_is_rejected() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        (
            "receiver.rs",
            "pub fn dispatch() { crate::neutral::wait_for_panel_activity(); }\n",
        ),
        (
            "neutral.rs",
            "pub fn wait_for_panel_activity() { std::thread::sleep(std::time::Duration::from_secs(30)); }\n",
        ),
    ]);

    assert_violation(fixture.path(), "blocking activity wait");
}

#[test]
fn receiver_reachable_selected_panel_controller_is_rejected() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod receiver;\n"),
        (
            "receiver.rs",
            "pub fn dispatch(app: &mut crate::tui::App) { let _ = app.active_brain_controller_mut(); }\n",
        ),
    ]);

    assert_violation(fixture.path(), "selected-panel controller access");
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
            "pub fn schedule(cleanups: &mut std::collections::VecDeque<String>, attachment_results: &std::sync::mpsc::Receiver<String>) { cleanups.push_back(String::new()); let _ = attachment_results.try_recv(); }\n",
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
            "pub struct Lease;\npub fn wait() { std::thread::sleep(std::time::Duration::from_millis(1)); }\n",
        ),
        (
            "fixture.rs",
            "pub fn consume(mut jobs: std::collections::VecDeque<crate::server::receiver::job::InboundJob>) { let _ = jobs.pop_front(); }\n",
        ),
    ]);

    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations.is_empty(),
        "valid non-receiver consumers must remain available: {violations:?}"
    );
}

fn assert_violation(root: &std::path::Path, expected: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(expected)),
        "forbidden mutation escaped the structural guard: {violations:?}"
    );
}
