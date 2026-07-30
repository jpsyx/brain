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
    StartNext,
}

#[must_use]
pub const fn dispatch_action(
    has_queued_message: bool,
    panel_open: bool,
    interactive_turn_active: bool,
    remote_job_active: bool,
) -> DispatchAction {
    if !has_queued_message || remote_job_active || interactive_turn_active {
        DispatchAction::WaitForTurn
    } else if panel_open {
        DispatchAction::CloseIdlePanel
    } else {
        DispatchAction::StartNext
    }
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

    #[test]
    fn queued_message_closes_an_idle_interactive_panel_before_dispatch() {
        assert_eq!(
            dispatch_action(true, true, false, false),
            DispatchAction::CloseIdlePanel
        );
    }

    #[test]
    fn queued_message_waits_for_an_active_interactive_turn() {
        assert_eq!(
            dispatch_action(true, true, true, false),
            DispatchAction::WaitForTurn
        );
    }

    #[test]
    fn queued_message_starts_when_no_panel_or_remote_job_is_active() {
        assert_eq!(
            dispatch_action(true, false, false, false),
            DispatchAction::StartNext
        );
        assert_eq!(
            dispatch_action(true, false, false, true),
            DispatchAction::WaitForTurn
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
        assert!(!retry_ready(
            Some(now + Duration::from_secs(5)),
            now
        ));
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
