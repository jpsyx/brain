use std::time::{Duration, Instant};

use super::{ReceiverRuntime, SyncGateObservation, SyncGatePoll};

#[test]
fn construction_has_no_sync_or_durable_run_state() {
    let runtime = ReceiverRuntime::new(false);

    assert!(!runtime.is_enabled());
    assert!(!runtime.sync_gate_is_armed());
}

#[test]
fn sync_gate_transitions_only_from_caller_supplied_observations() {
    let mut runtime = ReceiverRuntime::new(true);
    let launched_at = Instant::now();
    runtime.arm_sync_gate(launched_at, Some(4), 1);

    let waiting = runtime.poll_sync_gate(SyncGateObservation::new(launched_at, Some(4), false));
    assert!(matches!(waiting, Some(SyncGatePoll::Waiting)));

    let completed = runtime.poll_sync_gate(SyncGateObservation::new(
        launched_at + Duration::from_millis(250),
        Some(5),
        false,
    ));
    assert!(matches!(completed, Some(SyncGatePoll::Completed)));
    assert!(!runtime.sync_gate_is_armed());
}
