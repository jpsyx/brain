use super::support::{assert_controller_violation, assert_no_violations, receiver_fixture};

#[test]
fn later_harmless_let_replaces_an_earlier_controller_fact() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { let value: crate::agent::controller::AgentController = unreachable!(); let value: Harmless = unreachable!(); value.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "controller shadowed by harmless let");
}

#[test]
fn later_controller_let_replaces_an_earlier_harmless_fact() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { let value: Harmless = unreachable!(); let value: crate::agent::controller::AgentController = unreachable!(); value.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "harmless shadowed by controller let");
}

#[test]
fn shadow_initializer_reads_the_previous_binding_before_replacement() {
    let fixture = receiver_fixture(
        "pub fn harmless(_: crate::agent::controller::AgentController) -> Harmless { Harmless }\npub fn dispatch() { let value: crate::agent::controller::AgentController = unreachable!(); let value = harmless(value); value.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "initializer-before-shadow ordering");
}

#[test]
fn harmless_or_pattern_replaces_an_earlier_controller_fact() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { let value: crate::agent::controller::AgentController = unreachable!(); let (value @ _ | value @ _): Harmless = unreachable!(); value.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "or-pattern lexical replacement");
}
