//! Pure timing and channel-selection rules for remote message sessions.

use std::time::{Duration, Instant};

use crate::server::receiver::Channel;

pub const INACTIVITY_LEASE: Duration = Duration::from_secs(180);

/// How long a dispatched message may go without a completion signal before its
/// turn is eligible to be abandoned. Short enough that a wedged turn does not
/// strand the messages queued behind it; a turn that is still visibly working
/// is never abandoned on this deadline alone.
pub const REMOTE_TURN_TIMEOUT: Duration = Duration::from_secs(300);

/// How long the panel must sit completely unchanged before the turn behind it
/// counts as stalled rather than slow.
///
/// Every frontend renders *something* while it works — a spinner, an elapsed
/// counter, streaming output — so a panel that has not changed in this long is
/// waiting on a person, not on a model. Deliberately generous: the cost of
/// calling a working turn stalled is killing a good answer, while the cost of
/// waiting another minute on a truly wedged one is only that minute.
pub const ACTIVE_WORK_IDLE: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    pub channel: Channel,
    pub generation: u64,
    pub deadline: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchAction {
    WaitForTurn,
    CloseIdlePanel,
    ReuseReceiverPanel,
    StartNext,
}

#[must_use]
pub fn dispatch_action_for_channel(
    queued_channel: Option<Channel>,
    panel_open: bool,
    panel_channel: Option<Channel>,
    turn_active: bool,
    remote_job_active: bool,
) -> DispatchAction {
    if queued_channel.is_none() || remote_job_active || turn_active {
        DispatchAction::WaitForTurn
    } else if panel_open && queued_channel == panel_channel {
        DispatchAction::ReuseReceiverPanel
    } else if panel_open {
        DispatchAction::CloseIdlePanel
    } else {
        DispatchAction::StartNext
    }
}

#[must_use]
pub const fn should_poll_interactive_completion(
    turn_active: bool,
    remote_job_active: bool,
) -> bool {
    turn_active && !remote_job_active
}

pub fn commit_dispatch<T>(queue: &mut Vec<T>, launch_succeeded: bool) -> Option<T> {
    (launch_succeeded && !queue.is_empty()).then(|| queue.remove(0))
}

#[must_use]
pub fn retry_ready(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_none_or(|deadline| now >= deadline)
}

#[must_use]
pub fn renew(channel: Channel, generation: u64, now: Instant) -> Lease {
    Lease {
        channel,
        generation,
        deadline: now + INACTIVITY_LEASE,
    }
}

/// When to sample the panel after a message is dispatched.
///
/// Spread so the samples answer different questions: whether the prompt was
/// accepted at all, whether a turn actually started, and what the screen looked
/// like once it plainly should have finished.
pub const PROBE_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(20),
    Duration::from_secs(60),
];

/// Deadline for the probe after `fired` of them, or `None` once sampling ends.
#[must_use]
pub fn next_probe(fired: usize, dispatched_at: Instant) -> Option<Instant> {
    PROBE_DELAYS.get(fired).map(|delay| dispatched_at + *delay)
}

/// Whether an in-flight remote turn should be given up on.
///
/// Nothing else releases a dispatched turn: the inactivity lease only expires
/// once no message is in flight, so a turn that never signals completion pins
/// the panel and every later message waits behind it indefinitely.
///
/// Two conditions, both required. The deadline has to pass, *and* the panel has
/// to have stopped changing. An agent that is still rendering work is being
/// waited on for a good reason, and no deadline should cut it off; `None`
/// activity means nothing has been observed working, so only the deadline
/// applies.
#[must_use]
pub fn abandons_stalled_turn(
    started: Option<Instant>,
    last_panel_change: Option<Instant>,
    now: Instant,
) -> bool {
    started.is_some_and(|started| now.saturating_duration_since(started) >= REMOTE_TURN_TIMEOUT)
        && last_panel_change.is_none_or(|changed| {
            now.saturating_duration_since(changed) >= ACTIVE_WORK_IDLE
        })
}

/// Whether a local keystroke may reach the brain PTY.
///
/// While a remote message is being answered the panel is the sender's
/// conversation, not the user's: a stray keystroke lands in the composer beside
/// the injected prompt, and an `Enter` submits it half-written. The interrupt
/// key stays live so a stuck remote turn can never trap the user.
#[must_use]
pub const fn forwards_local_keystroke(remote_turn_in_flight: bool, interrupt: bool) -> bool {
    interrupt || !remote_turn_in_flight
}

#[must_use]
pub fn expired(
    lease: Lease,
    now: Instant,
    active_channel: Option<Channel>,
    generation: u64,
) -> bool {
    lease.deadline <= now && active_channel == Some(lease.channel) && generation == lease.generation
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SMS and email are the same decision problem: the dispatcher compares
    /// channels for equality and never matches a variant, so every scenario must
    /// resolve identically whichever channel is inbound. Asserted for both
    /// because the wait/reuse/start rules had only ever been proven for SMS.
    #[test]
    fn every_dispatch_decision_is_identical_for_sms_and_email() {
        for (label, panel_open, turn_active, remote_job_active, expected) in [
            ("idle panel", true, false, false, DispatchAction::CloseIdlePanel),
            ("active turn", false, true, false, DispatchAction::WaitForTurn),
            ("remote job", false, false, true, DispatchAction::WaitForTurn),
            ("nothing running", false, false, false, DispatchAction::StartNext),
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
            dispatch_action_for_channel(
                Some(Channel::Email),
                true,
                Some(Channel::Sms),
                false,
                false,
            ),
            DispatchAction::CloseIdlePanel
        );
    }

    #[test]
    fn failed_agent_launch_keeps_the_message_queued_for_retry() {
        let mut queue = vec!["first", "second"];
        commit_dispatch(&mut queue, false);
        assert_eq!(queue, vec!["first", "second"]);

        commit_dispatch(&mut queue, true);
        assert_eq!(queue, vec!["second"]);
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
        assert!(first < second, "probes must spread out, not repeat instantly");
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
}
