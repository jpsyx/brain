
#[test]
fn route_lookup_cannot_consume_expiry_before_watchdog_revokes_admission() {
    run_late_revocation(LateRevocation::RouteLookupThenExpire);
}

#[test]
fn exact_lease_expiry_rejects_commit_without_waiting_for_watchdog_tick() {
    run_late_revocation(LateRevocation::ExpireBeforeCommitWithoutWatchdog);
}

#[test]
fn commit_intent_reload_crossing_exact_expiry_rejects_before_durable_admission() {
    run_late_revocation(LateRevocation::ExpireDuringCommitIntentReload);
}

#[test]
fn commit_waiting_for_control_samples_exact_expiry_inside_the_lock() {
    run_late_revocation(LateRevocation::ExpireWhileCommitWaitsForControl);
}
