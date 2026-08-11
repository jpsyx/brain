//! Pure timing and channel-selection rules for remote message sessions.

use std::time::{Duration, Instant};

use crate::server::receiver::Channel;

pub const INACTIVITY_LEASE: Duration = Duration::from_secs(180);

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
