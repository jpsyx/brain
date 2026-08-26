fn snapshot(state: ReceiverJobState) -> ReceiverRecoverySnapshot {
    ReceiverRecoverySnapshot {
        state,
        attempt_kind: ReceiverAttemptKind::Ordinary,
        launch_attempt_count: 0,
        recovery_count: 0,
        now_unix_ms: 999,
        launch_expires_at_unix_ms: Some(1_000),
        acceptance_expires_at_unix_ms: Some(1_000),
        progress_expires_at_unix_ms: Some(1_000),
        recovery_expires_at_unix_ms: None,
        absolute_work_expires_at_unix_ms: Some(2_000),
    }
}

#[test]
fn recovery_policy_waits_before_each_deadline_and_for_nonrecoverable_states() {
    for state in [
        ReceiverJobState::Queued,
        ReceiverJobState::Claimed,
        ReceiverJobState::Launching,
        ReceiverJobState::Launched,
        ReceiverJobState::Accepted,
        ReceiverJobState::Processing,
        ReceiverJobState::Retrying,
        ReceiverJobState::Failed,
        ReceiverJobState::Done,
    ] {
        assert_eq!(
            decide_receiver_recovery(snapshot(state)),
            ReceiverRecoveryDecision::Wait,
            "unexpected decision for {state:?}"
        );
    }
}

#[test]
fn recovery_policy_treats_deadline_equality_as_expired_for_every_active_phase() {
    for (state, expected) in [
        (
            ReceiverJobState::Claimed,
            ReceiverRecoveryDecision::RequeuePreAcceptance,
        ),
        (
            ReceiverJobState::Launching,
            ReceiverRecoveryDecision::RequeuePreAcceptance,
        ),
        (
            ReceiverJobState::Launched,
            ReceiverRecoveryDecision::RequeuePreAcceptance,
        ),
        (
            ReceiverJobState::Accepted,
            ReceiverRecoveryDecision::RecoverSameSession,
        ),
        (
            ReceiverJobState::Processing,
            ReceiverRecoveryDecision::RecoverSameSession,
        ),
    ] {
        let mut snapshot = snapshot(state);
        snapshot.now_unix_ms = 1_000;
        assert_eq!(
            decide_receiver_recovery(snapshot),
            expected,
            "deadline equality did not expire {state:?}"
        );
    }
}

#[test]
fn recovery_policy_keeps_launch_and_accepted_recovery_budgets_separate() {
    let mut last_ordinary_launch = snapshot(ReceiverJobState::Launched);
    last_ordinary_launch.now_unix_ms = 1_000;
    last_ordinary_launch.launch_attempt_count = MAX_RECEIVER_LAUNCH_ATTEMPTS - 1;
    assert_eq!(
        decide_receiver_recovery(last_ordinary_launch),
        ReceiverRecoveryDecision::TerminalFailure
    );

    let mut accepted = snapshot(ReceiverJobState::Accepted);
    accepted.now_unix_ms = 1_000;
    accepted.launch_attempt_count = MAX_RECEIVER_LAUNCH_ATTEMPTS;
    assert_eq!(
        decide_receiver_recovery(accepted),
        ReceiverRecoveryDecision::RecoverSameSession,
        "pre-acceptance launch exhaustion must not consume accepted recovery"
    );

    let mut recovery = snapshot(ReceiverJobState::Accepted);
    recovery.now_unix_ms = 1_000;
    recovery.attempt_kind = ReceiverAttemptKind::Recovery;
    recovery.recovery_count = MAX_RECEIVER_RECOVERY_ATTEMPTS;
    assert_eq!(
        decide_receiver_recovery(recovery),
        ReceiverRecoveryDecision::TerminalFailure,
        "a recovered accepted job may not launch a second recovery"
    );
}

#[test]
fn recovery_policy_fails_closed_at_the_recovery_or_absolute_limit() {
    for (recovery_expires_at_unix_ms, absolute_work_expires_at_unix_ms) in
        [(Some(1_000), Some(2_000)), (None, Some(1_000))]
    {
        let mut accepted = snapshot(ReceiverJobState::Accepted);
        accepted.now_unix_ms = 1_000;
        accepted.progress_expires_at_unix_ms = Some(2_000);
        accepted.recovery_expires_at_unix_ms = recovery_expires_at_unix_ms;
        accepted.absolute_work_expires_at_unix_ms = absolute_work_expires_at_unix_ms;
        assert_eq!(
            decide_receiver_recovery(accepted),
            ReceiverRecoveryDecision::TerminalFailure
        );
    }
}

#[test]
fn recovery_policy_terminalizes_an_unclaimed_recovery_at_its_lifetime_boundaries() {
    let mut recovery = snapshot(ReceiverJobState::Retrying);
    recovery.attempt_kind = ReceiverAttemptKind::Recovery;
    recovery.recovery_count = MAX_RECEIVER_RECOVERY_ATTEMPTS;
    recovery.progress_expires_at_unix_ms = None;
    recovery.recovery_expires_at_unix_ms = Some(1_000);
    recovery.absolute_work_expires_at_unix_ms = Some(2_000);
    assert_eq!(
        decide_receiver_recovery(recovery),
        ReceiverRecoveryDecision::Wait
    );

    recovery.now_unix_ms = 1_000;
    assert_eq!(
        decide_receiver_recovery(recovery),
        ReceiverRecoveryDecision::TerminalFailure
    );

    recovery.recovery_expires_at_unix_ms = Some(3_000);
    recovery.absolute_work_expires_at_unix_ms = Some(1_000);
    assert_eq!(
        decide_receiver_recovery(recovery),
        ReceiverRecoveryDecision::TerminalFailure
    );
}

#[test]
fn recovery_policy_identifies_incomplete_legacy_completion_states() {
    for state in [
        ReceiverJobState::AnswerReady,
        ReceiverJobState::Delivering,
    ] {
        assert_eq!(
            decide_receiver_recovery(snapshot(state)),
            ReceiverRecoveryDecision::IncompleteLegacyCompletion
        );
    }
}

#[test]
fn lifecycle_deadlines_use_authorization_time_and_saturating_arithmetic() {
    let deadlines = ReceiverLifecycleDeadlines::after_acceptance(1_000, u64::MAX);
    assert_eq!(deadlines.progress_expires_at_unix_ms, 301_000);
    assert_eq!(deadlines.absolute_work_expires_at_unix_ms, 1_801_000);
    assert_eq!(deadlines.latest_progress_at_unix_ms, None);

    let progressed = deadlines.after_progress(2_000, u64::MAX);
    assert_eq!(progressed.progress_expires_at_unix_ms, 302_000);
    assert_eq!(progressed.absolute_work_expires_at_unix_ms, 1_801_000);
    assert_eq!(progressed.latest_progress_at_unix_ms, Some(u64::MAX));

    assert_eq!(
        receiver_launch_expires_at(u64::MAX - 1),
        u64::MAX,
        "launch arithmetic must saturate"
    );
    assert_eq!(
        receiver_acceptance_expires_at(u64::MAX - 1),
        u64::MAX,
        "acceptance arithmetic must saturate"
    );
}
