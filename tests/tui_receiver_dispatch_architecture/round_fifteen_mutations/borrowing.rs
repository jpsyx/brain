use super::support::{
    assert_channel_consumer, assert_no_violations, assert_queue_consumer, receiver_fixture,
};

const QUEUE: &str = "std::collections::VecDeque<crate::server::receiver::job::InboundJob>";

#[test]
fn borrowed_tuple_projection_keeps_the_queue_borrowed() {
    let fixture = receiver_fixture(&format!(
        "pub fn inspect((queue,): &({QUEUE},)) {{ let _ = queue.into_iter(); }}\n"
    ));
    assert_no_violations(fixture.path(), "borrowed tuple projection");
}

#[test]
fn owned_tuple_projection_still_consumes_the_queue() {
    let fixture = receiver_fixture(&format!(
        "pub fn consume((queue,): ({QUEUE},)) {{ let _ = queue.into_iter(); }}\n"
    ));
    assert_queue_consumer(fixture.path(), "owned tuple projection");
}

#[test]
fn borrowed_struct_match_ergonomics_keep_the_field_borrowed() {
    let fixture = receiver_fixture(&format!(
        "pub struct Inputs {{ pub queue: {QUEUE} }}\npub fn inspect(Inputs {{ queue }}: &Inputs) {{ let _ = queue.into_iter(); }}\n"
    ));
    assert_no_violations(fixture.path(), "borrowed struct match ergonomics");
}

#[test]
fn explicit_ref_binding_borrows_an_owned_struct_field() {
    let fixture = receiver_fixture(&format!(
        "pub struct Inputs {{ pub queue: {QUEUE} }}\npub fn inspect(Inputs {{ ref queue }}: Inputs) {{ let _ = queue.into_iter(); }}\n"
    ));
    assert_no_violations(fixture.path(), "explicit ref binding");
}

#[test]
fn explicit_ref_mut_binding_borrows_an_owned_struct_field() {
    let fixture = receiver_fixture(&format!(
        "pub struct Inputs {{ pub queue: {QUEUE} }}\npub fn inspect(Inputs {{ ref mut queue }}: Inputs) {{ let _ = queue.into_iter(); }}\n"
    ));
    assert_no_violations(fixture.path(), "explicit ref mut binding");
}

#[test]
fn owned_struct_pattern_still_consumes_the_queue() {
    let fixture = receiver_fixture(&format!(
        "pub struct Inputs {{ pub queue: {QUEUE} }}\npub fn consume(Inputs {{ queue }}: Inputs) {{ let _ = queue.into_iter(); }}\n"
    ));
    assert_queue_consumer(fixture.path(), "owned struct pattern");
}

#[test]
fn borrowed_receiver_iteration_remains_message_consumption() {
    let fixture = receiver_fixture(
        "pub fn consume((channel,): &(std::sync::mpsc::Receiver<crate::server::receiver::job::InboundJob>,)) { let _ = channel.iter(); }\n",
    );
    assert_channel_consumer(fixture.path(), "borrowed Receiver iteration");
}
