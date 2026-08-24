use super::{fixture_receiver_violations, rust_fixture};

const SAFE_IMPL: &str = "impl Safe for Worker<'_> { fn drive(&mut self) {} }\n";
const UNSAFE_IMPL: &str = "impl Unsafe for Worker<'_> { fn drive(&mut self) { crate::neutral::inject(self.controller); } }\n";

#[test]
fn qualified_trait_calls_stay_distinct_when_safe_impl_precedes_unsafe_impl() {
    assert_qualified_trait_calls_are_isolated(&format!("{SAFE_IMPL}{UNSAFE_IMPL}"));
}

#[test]
fn qualified_trait_calls_stay_distinct_when_unsafe_impl_precedes_safe_impl() {
    assert_qualified_trait_calls_are_isolated(&format!("{UNSAFE_IMPL}{SAFE_IMPL}"));
}

fn assert_qualified_trait_calls_are_isolated(implementations: &str) {
    let neutral = neutral_source(implementations);
    let unsafe_receiver = receiver_source("Unsafe");
    let unsafe_fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        ("neutral.rs", &neutral),
        ("receiver.rs", &unsafe_receiver),
    ]);
    let unsafe_violations = fixture_receiver_violations(unsafe_fixture.path());
    assert!(
        unsafe_violations
            .iter()
            .any(|violation| violation.contains("AgentController submit_now")),
        "the qualified Unsafe implementation must retain its indirect forbidden edge: {unsafe_violations:?}"
    );

    let safe_receiver = receiver_source("Safe");
    let safe_fixture = rust_fixture(&[
        ("lib.rs", "mod neutral;\nmod receiver;\n"),
        ("neutral.rs", &neutral),
        ("receiver.rs", &safe_receiver),
    ]);
    let safe_violations = fixture_receiver_violations(safe_fixture.path());
    assert!(
        safe_violations.is_empty(),
        "the qualified Safe implementation must not inherit the Unsafe edge: {safe_violations:?}"
    );
}

fn neutral_source(implementations: &str) -> String {
    format!(
        "pub trait Safe {{ fn drive(&mut self); }}\n\
         pub trait Unsafe {{ fn drive(&mut self); }}\n\
         pub struct Worker<'a> {{ controller: &'a mut crate::AgentController }}\n\
         {implementations}\
         pub fn inject(controller: &mut crate::AgentController) {{ controller.submit_now(); }}\n"
    )
}

fn receiver_source(selected_trait: &str) -> String {
    format!(
        "use crate::neutral::{{{selected_trait}, Worker}};\n\
         pub fn dispatch(worker: &mut Worker<'_>) {{ <Worker<'_> as {selected_trait}>::drive(worker); }}\n"
    )
}
