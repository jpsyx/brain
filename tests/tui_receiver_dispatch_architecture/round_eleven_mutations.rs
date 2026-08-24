use super::{fixture_receiver_violations, rust_fixture};

#[test]
fn bare_const_path_does_not_consume_a_later_type_default() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4; pub fn dispatch() { type Local<const N: usize, T = crate::agent::controller::AgentController> = T; let controller: &mut Local<CAP> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn lifetime_and_bare_const_path_keep_the_type_default_aligned() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4; pub fn dispatch() { type Local<'a, const N: usize, T = crate::agent::controller::AgentController> = T; let controller: &mut Local<'static, CAP> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn braced_const_expression_keeps_the_type_default_aligned() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4; pub fn dispatch() { type Local<const N: usize, T = crate::agent::controller::AgentController> = T; let controller: &mut Local<{ CAP + 1 }> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn literal_const_keeps_the_type_default_aligned() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<const N: usize, T = crate::agent::controller::AgentController> = T; let controller: &mut Local<4> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn explicit_type_after_a_bare_const_path_overrides_the_default() {
    let fixture = receiver_fixture(
        "pub const CAP: usize = 4; pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local<const N: usize, T = crate::agent::controller::AgentController> = T; let controller: &mut Local<CAP, Harmless> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an explicit harmless type after a const path must override the default",
    );
}

#[test]
fn const_value_named_like_the_brain_controller_is_not_a_type_argument() {
    let fixture = agent_fixture(
        "use crate::agent::*; pub const AgentController: usize = 4; pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local<const N: usize, T = Harmless> = T; let controller: &mut Local<AgentController> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "a const value in a const position must not acquire the imported type identity",
    );
}

#[test]
fn finite_same_alias_nesting_resolves_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T = crate::agent::controller::AgentController> = T; let controller: &mut Local<Local<crate::agent::controller::AgentController>> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn deeper_finite_same_alias_nesting_resolves_the_brain_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T = crate::agent::controller::AgentController> = T; let controller: &mut Local<Local<Local<crate::agent::controller::AgentController>>> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_has_controller_violation(fixture.path());
}

#[test]
fn finite_same_alias_nesting_with_a_harmless_type_stays_harmless() {
    let fixture = receiver_fixture(
        "pub struct Harmless; impl Harmless { pub fn submit_now(&mut self) {} } pub fn dispatch() { type Local<T> = T; let controller: &mut Local<Local<Harmless>> = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "finite same-alias nesting must preserve an explicit harmless identity",
    );
}

#[test]
fn explicit_self_cycle_terminates_without_a_false_positive() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type Local<T = Local> = T; let controller: &mut Local = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "an explicit self-cycle must terminate without inventing a Brain type",
    );
}

#[test]
fn mutual_cycle_terminates_without_a_false_positive() {
    let fixture = receiver_fixture(
        "pub fn dispatch() { type First<T = Second> = T; type Second<U = First> = U; let controller: &mut First = unreachable!(); controller.submit_now(); }\n",
    );

    assert_no_violations(
        fixture.path(),
        "a mutual alias cycle must terminate without inventing a Brain type",
    );
}

fn receiver_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[("lib.rs", "mod receiver;\n"), ("receiver.rs", receiver)])
}

fn agent_fixture(receiver: &str) -> tempfile::TempDir {
    rust_fixture(&[
        ("lib.rs", "mod agent;\nmod receiver;\n"),
        ("agent.rs", "pub mod controller;\npub use controller::*;\n"),
        (
            "agent/controller.rs",
            "pub struct AgentController; impl AgentController { pub fn submit_now(&mut self) {} }\n",
        ),
        ("receiver.rs", receiver),
    ])
}

fn assert_has_controller_violation(root: &std::path::Path) {
    let violations = fixture_receiver_violations(root);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "the canonical controller identity must remain guarded: {violations:?}"
    );
}

fn assert_no_violations(root: &std::path::Path, message: &str) {
    let violations = fixture_receiver_violations(root);
    assert!(violations.is_empty(), "{message}: {violations:?}");
}
