
#[test]
fn commit_cas_linearizes_while_control_mutex_is_held() {
    run_late_revocation(LateRevocation::CommitLinearizesUnderControl);
}

