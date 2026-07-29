//! Pure timing and channel-selection rules for remote message sessions.

use std::time::{Duration, Instant};

use crate::server::messaging::Channel;

pub const INACTIVITY_LEASE: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    pub channel: Channel,
    pub generation: u64,
    pub deadline: Instant,
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
