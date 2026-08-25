use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn dangerous_first_cfg_return_survives_for_direct_recv() {
    let fixture = cfg_receiver_return_fixture(true, "recv");
    assert_channel_consumer(fixture.path(), "dangerous-first cfg return");
}

#[test]
fn dangerous_last_cfg_return_survives_for_direct_iter() {
    let fixture = cfg_receiver_return_fixture(false, "iter");
    assert_channel_consumer(fixture.path(), "dangerous-last cfg return");
}

#[test]
fn cfg_return_alternatives_preserve_controller_role_propagation() {
    let fixture = controller_return_fixture();
    assert_controller_violation(fixture.path(), "cfg controller return");
}

#[test]
fn exact_cfg_test_return_alternative_remains_excluded() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod server;\n"),
        (
            "neutral.rs",
            "#[cfg(test)]\nfn source() -> std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob> { todo!() }\n#[cfg(not(test))]\nfn source() -> std::sync::mpsc::Receiver<String> { todo!() }\npub fn inspect() { let _ = source().recv(); }\n",
        ),
        ("server.rs", inbound_job_source()),
    ]);

    assert_no_violations(fixture.path(), "exact cfg(test) return alternative");
}

#[test]
fn separate_cfg_return_facts_do_not_invent_a_combined_consumer() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod server;\n"),
        (
            "neutral.rs",
            "pub struct Inbox<T>(T);\nimpl<T> Inbox<T> { pub fn recv(self) {} }\n#[cfg(target_os = \"linux\")]\nfn source() -> std::sync::mpsc::Receiver<String> { todo!() }\n#[cfg(target_os = \"macos\")]\nfn source() -> Inbox<crate::server::receiver::job::InboundJob> { todo!() }\npub fn inspect() { source().recv(); }\n",
        ),
        ("server.rs", inbound_job_source()),
    ]);

    assert_no_violations(fixture.path(), "non-combinable cfg return roles");
}

#[test]
fn tuple_function_parameter_binds_only_matching_components() {
    let fixture = destructuring_fixture(
        "pub fn dispatch((controller, harmless): (&mut crate::agent::controller::AgentController, &mut Harmless)) { controller.submit_now(); harmless.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "tuple function parameter");
}

#[test]
fn harmless_tuple_component_does_not_inherit_controller_role() {
    let fixture = destructuring_fixture(
        "pub fn dispatch((_controller, harmless): (&mut crate::agent::controller::AgentController, &mut Harmless)) { harmless.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "harmless tuple component");
}

#[test]
fn nested_tuple_function_parameter_preserves_each_role() {
    let fixture = destructuring_fixture(
        "pub fn dispatch((_label, (controller, mut queue)): (String, (&mut crate::agent::controller::AgentController, std::collections::VecDeque<crate::server::receiver::job::InboundJob>))) { controller.submit_now(); let _ = queue.pop_front(); }\n",
    );
    let violations = fixture_receiver_violations(fixture.path());
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "nested controller component must remain forbidden: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver queue consume")),
        "nested queue component must remain a global consumer: {violations:?}"
    );
}

#[test]
fn typed_closure_tuple_parameter_binds_matching_components() {
    let fixture = destructuring_fixture(
        "pub fn dispatch() { let invoke = |(harmless, controller): (&mut Harmless, &mut crate::agent::controller::AgentController)| { harmless.submit_now(); controller.submit_now(); }; let _ = invoke; }\n",
    );
    assert_controller_violation(fixture.path(), "tuple closure parameter");
}

#[test]
fn nested_closure_tuple_harmless_component_stays_harmless() {
    let fixture = destructuring_fixture(
        "pub fn dispatch() { let invoke = |((_controller, harmless), _label): ((&mut crate::agent::controller::AgentController, &mut Harmless), String)| harmless.submit_now(); let _ = invoke; }\n",
    );
    assert_no_violations(fixture.path(), "nested harmless closure component");
}

#[test]
fn struct_and_slice_patterns_retain_their_component_facts() {
    let fixture = destructuring_fixture(
        "pub struct Inputs<'a> { pub controller: &'a mut crate::agent::controller::AgentController, pub harmless: &'a mut Harmless }\npub fn dispatch(Inputs { controller, harmless }: Inputs<'_>, [first, _]: [&mut crate::agent::controller::AgentController; 2]) { harmless.submit_now(); controller.submit_now(); first.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "struct and slice components");
}

#[test]
fn reference_wrapped_tuple_pattern_retains_inner_facts() {
    let fixture = destructuring_fixture(
        "pub fn dispatch(&(controller, _label): &(crate::agent::controller::AgentController, String)) { controller.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "reference-wrapped tuple pattern");
}

#[test]
fn tuple_struct_pattern_binds_fields_by_position() {
    let fixture = destructuring_fixture(
        "pub struct Inputs<'a>(pub &'a mut crate::agent::controller::AgentController, pub &'a mut Harmless);\npub fn dispatch(Inputs(controller, harmless): Inputs<'_>) { harmless.submit_now(); controller.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "tuple struct fields");
}

#[test]
fn or_pattern_preserves_the_shared_binding_fact() {
    let fixture = destructuring_fixture(
        "pub fn dispatch((controller @ _ | controller @ _): &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "or-pattern shared binding");
}

fn cfg_receiver_return_fixture(dangerous_first: bool, operation: &str) -> tempfile::TempDir {
    let dangerous = "#[cfg(target_os = \"linux\")]\nfn source() -> std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob> { todo!() }\n";
    let safe = "#[cfg(target_os = \"macos\")]\nfn source() -> std::sync::mpsc::Receiver<String> { todo!() }\n";
    let alternatives = if dangerous_first {
        format!("{dangerous}{safe}")
    } else {
        format!("{safe}{dangerous}")
    };
    let neutral = format!("{alternatives}pub fn drain() {{ let _ = source().{operation}(); }}\n");
    rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod server;\n"),
        ("neutral.rs", &neutral),
        ("server.rs", inbound_job_source()),
    ])
}

fn controller_return_fixture() -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(self) {} }\n",
        ),
        (
            "receiver.rs",
            "pub struct Harmless;\nimpl Harmless { pub fn submit_now(self) {} }\n#[cfg(target_os = \"linux\")]\nfn source() -> crate::agent::controller::AgentController { todo!() }\n#[cfg(target_os = \"macos\")]\nfn source() -> Harmless { todo!() }\npub fn dispatch() { source().submit_now(); }\n",
        ),
    ])
}

fn destructuring_fixture(receiver: &str) -> tempfile::TempDir {
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
        ("server.rs", inbound_job_source()),
    ])
}

fn inbound_job_source() -> &'static str {
    "pub mod receiver { pub mod job { pub struct InboundJob; } }\n"
}

fn assert_controller_violation(root: &std::path::Path, case: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "{case} must retain the forbidden controller operation: {violations:?}"
    );
}

fn assert_channel_consumer(root: &std::path::Path, case: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("receiver channel consume")),
        "{case} must retain the inbound channel consumer: {violations:?}"
    );
}

fn assert_no_violations(root: &std::path::Path, case: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations.is_empty(),
        "{case} must remain outside the forbidden graph: {violations:?}"
    );
}
