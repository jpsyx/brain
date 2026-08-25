use super::super::{fixture_receiver_violations, rust_fixture};

pub(super) fn receiver_fixture(receiver: &str) -> tempfile::TempDir {
    let receiver = format!(
        "pub struct Harmless;\nimpl Harmless {{ pub fn submit_now(&mut self) {{}} }}\n{receiver}"
    );
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\nmod server;\n"),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("receiver.rs", &receiver),
        (
            "server.rs",
            "pub mod receiver { pub mod job { pub struct InboundJob; } }\n",
        ),
    ])
}

pub(super) fn assert_controller_violation(root: &std::path::Path, case: &str) {
    assert_violation(root, case, "AgentController submit_now");
}

pub(super) fn assert_channel_consumer(root: &std::path::Path, case: &str) {
    assert_violation(root, case, "receiver channel consume");
}

pub(super) fn assert_queue_consumer(root: &std::path::Path, case: &str) {
    assert_violation(root, case, "receiver queue consume");
}

pub(super) fn assert_no_violations(root: &std::path::Path, case: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations.is_empty(),
        "{case} must remain outside the forbidden graph: {violations:?}"
    );
}

fn assert_violation(root: &std::path::Path, case: &str, expected: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(expected)),
        "{case} must retain {expected}: {violations:?}"
    );
}
