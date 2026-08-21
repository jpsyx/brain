use super::*;

/// SMS and email are the same decision problem: the dispatcher compares
/// channels for equality and never matches a variant, so every scenario must
/// resolve identically whichever channel is inbound. Asserted for both
/// because the wait/reuse/start rules had only ever been proven for SMS.
#[test]
fn every_dispatch_decision_is_identical_for_sms_and_email() {
    for (label, panel_open, turn_active, remote_job_active, expected) in [
        (
            "idle panel",
            true,
            false,
            false,
            DispatchAction::CloseIdlePanel,
        ),
        (
            "active turn",
            false,
            true,
            false,
            DispatchAction::WaitForTurn,
        ),
        (
            "remote job",
            false,
            false,
            true,
            DispatchAction::WaitForTurn,
        ),
        (
            "nothing running",
            false,
            false,
            false,
            DispatchAction::StartNext,
        ),
    ] {
        for channel in [Channel::Sms, Channel::Email] {
            assert_eq!(
                dispatch_action_for_channel(
                    Some(channel),
                    panel_open,
                    None,
                    turn_active,
                    remote_job_active,
                ),
                expected,
                "{label} differed for {channel:?}"
            );
        }
    }
}

/// A warm panel is reused for its own channel and replaced for the other,
/// symmetrically in both directions.
#[test]
fn warm_panel_reuse_and_replacement_are_symmetric_across_channels() {
    for (queued, panel) in [
        (Channel::Sms, Channel::Sms),
        (Channel::Email, Channel::Email),
    ] {
        assert_eq!(
            dispatch_action_for_channel(Some(queued), true, Some(panel), false, false),
            DispatchAction::ReuseReceiverPanel,
            "{queued:?} should reuse its own warm panel"
        );
    }
    for (queued, panel) in [
        (Channel::Sms, Channel::Email),
        (Channel::Email, Channel::Sms),
    ] {
        assert_eq!(
            dispatch_action_for_channel(Some(queued), true, Some(panel), false, false),
            DispatchAction::CloseIdlePanel,
            "{queued:?} should replace a {panel:?} panel"
        );
    }
}

#[test]
fn queued_message_closes_an_idle_interactive_panel_before_dispatch() {
    assert_eq!(
        dispatch_action_for_channel(Some(Channel::Sms), true, None, false, false),
        DispatchAction::CloseIdlePanel
    );
}

#[test]
fn queued_message_waits_for_an_active_interactive_turn() {
    assert_eq!(
        dispatch_action_for_channel(Some(Channel::Sms), true, None, true, false),
        DispatchAction::WaitForTurn
    );
}

#[test]
fn queued_message_starts_when_no_panel_or_remote_job_is_active() {
    assert_eq!(
        dispatch_action_for_channel(Some(Channel::Sms), false, None, false, false),
        DispatchAction::StartNext
    );
    assert_eq!(
        dispatch_action_for_channel(Some(Channel::Sms), false, None, false, true),
        DispatchAction::WaitForTurn
    );
}

#[test]
fn warm_receiver_lease_does_not_hide_interactive_turn_completion() {
    assert!(should_poll_interactive_completion(true, false));
    assert!(!should_poll_interactive_completion(true, true));
    assert!(!should_poll_interactive_completion(false, false));
}

#[test]
fn queued_message_reuses_a_warm_panel_for_the_same_channel() {
    assert_eq!(
        dispatch_action_for_channel(Some(Channel::Sms), true, Some(Channel::Sms), false, false,),
        DispatchAction::ReuseReceiverPanel
    );
}

#[test]
fn queued_message_replaces_a_warm_panel_for_a_different_channel() {
    assert_eq!(
        dispatch_action_for_channel(Some(Channel::Email), true, Some(Channel::Sms), false, false,),
        DispatchAction::CloseIdlePanel
    );
}

#[test]
fn failed_launch_retry_waits_for_its_backoff_deadline() {
    let now = Instant::now();
    assert!(retry_ready(None, now));
    assert!(!retry_ready(Some(now + Duration::from_secs(5)), now));
    assert!(retry_ready(
        Some(now + Duration::from_secs(5)),
        now + Duration::from_secs(5)
    ));
}

/// A prompt that sits unsubmitted in the composer is indistinguishable, from
/// brain's side, from a slow tool call: both are simply "no completion yet".
/// The panel has to be sampled while the turn is still open, and more than
/// once, or the evidence is gone by the time anyone looks.
#[test]
fn an_open_turn_is_sampled_repeatedly_and_then_stops() {
    let now = Instant::now();
    let first = next_probe(0, now).expect("a turn is sampled soon after dispatch");
    let second = next_probe(1, now).expect("and again once it should have started");
    assert!(
        first < second,
        "probes must spread out, not repeat instantly"
    );
    assert!(
        next_probe(PROBE_DELAYS.len(), now).is_none(),
        "sampling stops rather than logging forever"
    );
    assert!(
        PROBE_DELAYS
            .last()
            .is_some_and(|last| *last < REMOTE_TURN_TIMEOUT),
        "every probe must land while the turn is still open"
    );
}

/// A turn that never signalled completion pinned the panel forever, so
/// every later message queued behind it received the processing notice and
/// nothing else. Once the deadline passes and nothing is happening on
/// screen, it is abandoned so the queue can drain.
#[test]
fn a_stalled_turn_is_abandoned_once_the_deadline_passes() {
    let now = Instant::now();
    let past_deadline = now + REMOTE_TURN_TIMEOUT;
    assert!(!abandons_stalled_turn(None, None, past_deadline));
    assert!(!abandons_stalled_turn(Some(now), None, now));
    assert!(abandons_stalled_turn(Some(now), None, past_deadline));
}

/// An agent that is still working is rightfully being waited on. Abandoning
/// it would kill a good answer mid-flight and tell the sender to resend
/// something that was about to arrive.
#[test]
fn a_turn_still_doing_visible_work_is_never_abandoned() {
    let now = Instant::now();
    let long_past_deadline = now + REMOTE_TURN_TIMEOUT + Duration::from_secs(3600);
    let just_moved = long_past_deadline
        .checked_sub(Duration::from_secs(1))
        .expect("the panel changed a moment ago");
    assert!(
        !abandons_stalled_turn(Some(now), Some(just_moved), long_past_deadline),
        "a turn whose panel is still moving must be left alone, however long it takes"
    );

    let went_quiet = long_past_deadline
        .checked_sub(ACTIVE_WORK_IDLE)
        .expect("the panel stopped changing");
    assert!(
        abandons_stalled_turn(Some(now), Some(went_quiet), long_past_deadline),
        "a panel that stopped moving is a stalled turn, not a slow one"
    );
}

/// The deadlines have to nest: a sender is told the answer is still coming
/// long before the turn is given up on, and "quiet" has to be shorter than
/// the deadline or it could never be observed.
#[test]
fn the_abandon_deadline_sits_between_the_processing_notice_and_giving_up() {
    assert_eq!(REMOTE_TURN_TIMEOUT, Duration::from_secs(300));
    assert!(REMOTE_TURN_TIMEOUT > Duration::from_secs(120));
    assert!(ACTIVE_WORK_IDLE < REMOTE_TURN_TIMEOUT);
}

/// Typing beside an in-flight remote answer corrupted the injected prompt,
/// and an `Enter` submitted it half-written, so the panel is locked for the
/// duration of the remote turn.
#[test]
fn local_keystrokes_are_locked_out_while_a_remote_turn_is_in_flight() {
    assert!(!forwards_local_keystroke(true, false));
    assert!(forwards_local_keystroke(false, false));
}

/// A remote turn that never completes must not leave the user unable to
/// reach their own agent.
#[test]
fn the_interrupt_key_is_never_locked_out() {
    assert!(forwards_local_keystroke(true, true));
    assert!(forwards_local_keystroke(false, true));
}

#[test]
fn receiving_a_message_renews_for_three_minutes() {
    let now = Instant::now();
    let lease = renew(Channel::Sms, 4, now);
    assert_eq!(lease.channel, Channel::Sms);
    assert_eq!(lease.generation, 4);
    assert_eq!(lease.deadline.duration_since(now), INACTIVITY_LEASE);
}

#[test]
fn an_old_generation_cannot_close_a_new_channel_session() {
    let now = Instant::now();
    let lease = renew(Channel::Sms, 4, now);
    assert!(!expired(
        lease,
        now + INACTIVITY_LEASE,
        Some(Channel::Sms),
        5
    ));
    assert!(!expired(
        lease,
        now + INACTIVITY_LEASE,
        Some(Channel::Email),
        4
    ));
}

#[test]
fn matching_channel_and_generation_expire() {
    let now = Instant::now();
    let lease = renew(Channel::Email, 8, now);
    assert!(expired(
        lease,
        now + INACTIVITY_LEASE,
        Some(Channel::Email),
        8
    ));
}
