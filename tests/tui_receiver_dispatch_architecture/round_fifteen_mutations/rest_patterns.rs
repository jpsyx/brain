use super::support::{
    assert_controller_violation, assert_no_violations, assert_queue_consumer, receiver_fixture,
};

const QUEUE: &str = "std::collections::VecDeque<crate::server::receiver::job::InboundJob>";

#[test]
fn tuple_suffix_after_rest_maps_from_the_end() {
    let fixture = receiver_fixture(&format!(
        "pub fn consume((_, .., mut queue): (Harmless, Harmless, {QUEUE})) {{ let _ = queue.pop_front(); }}\n"
    ));
    assert_queue_consumer(fixture.path(), "tuple suffix after rest");
}

#[test]
fn harmless_tuple_suffix_does_not_inherit_a_skipped_controller() {
    let fixture = receiver_fixture(
        "pub fn dispatch((_, .., value): (Harmless, crate::agent::controller::AgentController, Harmless)) { value.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "harmless tuple suffix");
}

#[test]
fn tuple_struct_suffix_after_rest_maps_from_the_end() {
    let fixture = receiver_fixture(&format!(
        "pub struct Holder(pub Harmless, pub Harmless, pub Harmless, pub {QUEUE});\npub fn consume(Holder(_, .., mut queue): Holder) {{ let _ = queue.pop_front(); }}\n"
    ));
    assert_queue_consumer(fixture.path(), "tuple-struct suffix after rest");
}

#[test]
fn harmless_tuple_struct_suffix_does_not_inherit_a_skipped_controller() {
    let fixture = receiver_fixture(
        "pub struct Holder(pub Harmless, pub Harmless, pub crate::agent::controller::AgentController, pub Harmless);\npub fn dispatch(Holder(_, .., value): Holder) { value.submit_now(); }\n",
    );
    assert_no_violations(fixture.path(), "harmless tuple-struct suffix");
}

#[test]
fn borrowed_slice_rest_binding_retains_sequence_shape_and_borrow() {
    let fixture = receiver_fixture(&format!(
        "pub fn inspect([_, tail @ ..]: &[{QUEUE}; 3]) {{ let _ = tail.into_iter(); }}\n"
    ));
    assert_no_violations(fixture.path(), "borrowed slice rest binding");
}

#[test]
fn owned_slice_rest_binding_can_project_an_owned_queue() {
    let fixture = receiver_fixture(&format!(
        "pub fn consume([_, tail @ ..]: [{QUEUE}; 3]) {{ let [queue, ..] = tail; let _ = queue.into_iter(); }}\n"
    ));
    assert_queue_consumer(fixture.path(), "owned slice rest projection");
}

#[test]
fn nested_reference_rest_projection_keeps_the_queue_borrowed() {
    let fixture = receiver_fixture(&format!(
        "pub fn inspect(([_, tail @ ..],): &([{QUEUE}; 3],)) {{ let [queue, ..] = tail; let _ = queue.into_iter(); }}\n"
    ));
    assert_no_violations(fixture.path(), "nested borrowed rest projection");
}

#[test]
fn generic_tuple_struct_rest_keeps_the_last_controller_field() {
    let fixture = receiver_fixture(
        "pub struct Holder<A, B, C>(pub A, pub B, pub Harmless, pub C);\npub fn dispatch(Holder(_, .., value): Holder<Harmless, Harmless, crate::agent::controller::AgentController>) { value.submit_now(); }\n",
    );
    assert_controller_violation(fixture.path(), "generic tuple-struct rest suffix");
}
