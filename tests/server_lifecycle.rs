#[path = "server_lifecycle/election.rs"]
mod election;
#[path = "server_lifecycle/process.rs"]
mod process;
#[path = "server_lifecycle/recovery.rs"]
mod recovery;
#[path = "server_lifecycle/support.rs"]
mod support;

#[test]
fn process_fixture_permit_bounds_concurrent_server_scenarios() {
    let permits = support::ProcessFixturePermits::new(2);
    let first = permits.acquire();
    let second = permits
        .try_acquire()
        .expect("two real-process scenarios may run concurrently");

    assert!(permits.try_acquire().is_none());
    drop(first);
    assert!(permits.try_acquire().is_some());
    drop(second);
}
