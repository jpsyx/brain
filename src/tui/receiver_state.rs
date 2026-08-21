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
        && last_panel_change
            .is_none_or(|changed| now.saturating_duration_since(changed) >= ACTIVE_WORK_IDLE)
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
mod tests;
