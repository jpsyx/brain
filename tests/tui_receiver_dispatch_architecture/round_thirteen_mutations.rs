use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn dangerous_platform_branch_survives_a_later_safe_same_id_branch() {
    let fixture = cfg_alternative_fixture(true);
    assert_controller_violation(fixture.path(), "dangerous-first platform alternative");
}

#[test]
fn dangerous_platform_branch_survives_an_earlier_safe_same_id_branch() {
    let fixture = cfg_alternative_fixture(false);
    assert_controller_violation(fixture.path(), "dangerous-last platform alternative");
}

#[test]
fn exact_cfg_test_same_id_branch_remains_excluded() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "receiver.rs",
            "#[cfg(test)]\nfn platform(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n#[cfg(not(test))]\nfn platform(_: &mut crate::agent::controller::AgentController) {}\npub fn dispatch(controller: &mut crate::agent::controller::AgentController) { platform(controller); }\n",
        ),
    ]);

    assert_no_violations(fixture.path(), "exact cfg(test) same-ID alternative");
}

#[test]
fn typed_closure_parameter_retains_the_controller_fact() {
    let fixture = closure_fixture(
        "pub fn dispatch() { let invoke = |controller: &mut crate::agent::controller::AgentController| controller.submit_now(); let _ = invoke; }\n",
    );
    assert_controller_violation(fixture.path(), "typed closure parameter");
}

#[test]
fn nested_move_closure_retains_the_inner_controller_fact() {
    let fixture = closure_fixture(
        "pub fn dispatch() { let nested = move || move |controller: &mut crate::agent::controller::AgentController| controller.submit_now(); let _ = nested; }\n",
    );
    assert_controller_violation(fixture.path(), "nested move closure");
}

#[test]
fn returned_closure_retains_its_controller_fact() {
    let fixture = closure_fixture(
        "pub fn dispatch() -> impl FnOnce(&mut crate::agent::controller::AgentController) { move |controller: &mut crate::agent::controller::AgentController| controller.submit_now() }\n",
    );
    assert_controller_violation(fixture.path(), "returned closure");
}

#[test]
fn harmless_typed_closure_and_nested_shadow_stay_local() {
    let fixture = closure_fixture(
        "pub fn dispatch() { let ordinary = |controller: &mut String| controller.push_str(\"ok\"); let nested = move || |controller: &mut String| controller.clear(); let _ = (ordinary, nested); }\n",
    );
    assert_no_violations(fixture.path(), "harmless String closure parameters");
}

#[test]
fn inbound_receiver_iter_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { let _ = inbox.iter(); }\n",
    );
    assert_channel_consumer(fixture.path(), "Receiver::iter");
}

#[test]
fn inbound_receiver_try_iter_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { let _ = inbox.try_iter(); }\n",
    );
    assert_channel_consumer(fixture.path(), "Receiver::try_iter");
}

#[test]
fn inbound_receiver_into_iter_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { let _ = inbox.into_iter(); }\n",
    );
    assert_channel_consumer(fixture.path(), "Receiver::into_iter");
}

#[test]
fn for_loop_over_an_inbound_receiver_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(inbox: std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>) { for _job in inbox {} }\n",
    );
    assert_channel_consumer(fixture.path(), "Receiver for loop");
}

#[test]
fn owned_inbound_vec_deque_into_iter_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(queue: std::collections::VecDeque<crate::server::receiver::job::InboundJob>) { let _ = queue.into_iter(); }\n",
    );
    assert_queue_consumer(fixture.path(), "owned VecDeque::into_iter");
}

#[test]
fn qualified_owned_inbound_vec_deque_into_iter_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(queue: std::collections::VecDeque<crate::server::receiver::job::InboundJob>) { let _ = <std::collections::VecDeque<crate::server::receiver::job::InboundJob> as IntoIterator>::into_iter(queue); }\n",
    );
    assert_queue_consumer(fixture.path(), "qualified owned VecDeque into_iter");
}

#[test]
fn for_loop_over_an_owned_inbound_vec_deque_is_a_global_consumer() {
    let fixture = inbound_collection_fixture(
        "pub fn drain(queue: std::collections::VecDeque<crate::server::receiver::job::InboundJob>) { for _job in queue {} }\n",
    );
    assert_queue_consumer(fixture.path(), "owned VecDeque for loop");
}

#[test]
fn borrowed_inbound_vec_deque_iteration_and_inspection_stay_harmless() {
    let fixture = inbound_collection_fixture(
        "pub fn inspect(queue: &std::collections::VecDeque<crate::server::receiver::job::InboundJob>) { let _ = queue.iter(); let _ = queue.front(); let _ = queue.len(); for _job in queue {} let _ = queue.into_iter(); }\n",
    );
    assert_no_violations(fixture.path(), "borrowed VecDeque iteration and inspection");
}

#[test]
fn unrelated_string_iterators_stay_harmless() {
    let fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\n"),
        (
            "neutral.rs",
            "pub fn ordinary(inbox: std::sync::mpsc::Receiver<String>, queue: std::collections::VecDeque<String>) { let _ = inbox.iter(); for _value in inbox {} let _ = queue.into_iter(); }\n",
        ),
    ]);
    assert_no_violations(fixture.path(), "unrelated String iterators");
}

fn cfg_alternative_fixture(dangerous_first: bool) -> tempfile::TempDir {
    let dangerous = "#[cfg(target_os = \"linux\")]\nfn platform(controller: &mut crate::agent::controller::AgentController) { crate::neutral::inject(controller); }\n";
    let safe = "#[cfg(target_os = \"macos\")]\nfn platform(_: &mut crate::agent::controller::AgentController) {}\n";
    let alternatives = if dangerous_first {
        format!("{dangerous}{safe}")
    } else {
        format!("{safe}{dangerous}")
    };
    let receiver = format!(
        "{alternatives}pub fn dispatch(controller: &mut crate::agent::controller::AgentController) {{ platform(controller); }}\n"
    );
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod neutral;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        (
            "neutral.rs",
            "pub fn inject(controller: &mut crate::agent::controller::AgentController) { controller.submit_now(); }\n",
        ),
        ("receiver.rs", &receiver),
    ])
}

fn closure_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController;\nimpl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("receiver.rs", receiver),
    ])
}

fn inbound_collection_fixture(neutral: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod server;\n"),
        ("neutral.rs", neutral),
        (
            "server.rs",
            "pub mod receiver { pub mod job { pub struct InboundJob; } }\n",
        ),
    ])
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
    assert_consumer(root, case, "receiver channel consume");
}

fn assert_queue_consumer(root: &std::path::Path, case: &str) {
    assert_consumer(root, case, "receiver queue consume");
}

fn assert_consumer(root: &std::path::Path, case: &str, expected: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(expected)),
        "{case} must be rejected as an inbound consumer: {violations:?}"
    );
}

fn assert_no_violations(root: &std::path::Path, case: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations.is_empty(),
        "{case} must remain outside the forbidden graph: {violations:?}"
    );
}
