#[test]
fn disable_after_final_revalidation_cancels_before_durable_admission() {
    run_late_revocation(LateRevocation::Disable);
}

#[test]
fn unregister_after_final_revalidation_cancels_before_durable_admission() {
    run_late_revocation(LateRevocation::Unregister);
}

#[test]
fn disable_enable_aba_after_final_revalidation_cancels_before_durable_admission() {
    run_late_revocation(LateRevocation::DisableEnableAba);
}

#[test]
fn watchdog_expiry_after_final_revalidation_cancels_before_durable_admission() {
    run_late_revocation(LateRevocation::Expire);
}
