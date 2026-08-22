//! Receiver-local runtime state and semantic transitions.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::actor::ActorContext;
use crate::server::receiver::{Channel, EmailReplyContext, InboundJob};

use super::InboundQueue;

#[cfg(test)]
use super::StageError;

mod diagnostics;
mod session;
mod sync;
mod tick;

pub(crate) use sync::{SyncGateObservation, SyncGatePoll};

const INACTIVITY_LEASE: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy)]
struct Lease {
    channel: Channel,
    generation: u64,
    deadline: Instant,
}

struct ReceiverSyncGate {
    seen_journal_id: Option<i64>,
    launched_at: Instant,
    next_poll: Instant,
    attempts: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRemoteTurn<'a> {
    pub(crate) response_id: &'a str,
    pub(crate) channel: Channel,
    pub(crate) sender: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteCompletionTarget {
    pub(crate) response_id: String,
    pub(crate) channel: Channel,
    pub(crate) sender: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DeliveryTarget {
    pub(crate) channel: Channel,
    pub(crate) sender: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EmailReplyTarget {
    pub(crate) response_email: Option<String>,
    pub(crate) recipients: Vec<String>,
    pub(crate) reply: Option<EmailReplyContext>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReceiverProbe {
    pub(crate) elapsed_seconds: u64,
    pub(crate) response_id: Option<String>,
}

pub(crate) struct ReceiverRuntime {
    socket: Option<crate::tui::singleton::JobSocket>,
    enabled: bool,
    queue: InboundQueue,
    new_session_channels: HashSet<Channel>,
    force_fresh: bool,
    requested_actor: Option<ActorContext>,
    lease: Option<Lease>,
    generation: u64,
    sender: Option<String>,
    recipients: Vec<String>,
    response_email: Option<String>,
    email_reply: Option<EmailReplyContext>,
    receiver_response_id: Option<String>,
    interactive_response_id: Option<String>,
    interactive_agent_session_id: Option<String>,
    resume_session: Option<String>,
    started: Option<Instant>,
    delay_sent: bool,
    probe: Option<(Instant, usize)>,
    panel_activity: Option<(u64, Instant)>,
    panel_sampled_at: Option<Instant>,
    retry_at: Option<Instant>,
    sync_gate: Option<ReceiverSyncGate>,
}

impl ReceiverRuntime {
    #[must_use]
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            socket: None,
            enabled,
            queue: InboundQueue::default(),
            new_session_channels: HashSet::new(),
            force_fresh: false,
            requested_actor: None,
            lease: None,
            generation: 0,
            sender: None,
            recipients: Vec::new(),
            response_email: None,
            email_reply: None,
            receiver_response_id: None,
            interactive_response_id: None,
            interactive_agent_session_id: None,
            resume_session: None,
            started: None,
            delay_sent: false,
            probe: None,
            panel_activity: None,
            panel_sampled_at: None,
            retry_at: None,
            sync_gate: None,
        }
    }

    pub(crate) fn install_socket(&mut self, socket: crate::tui::singleton::JobSocket) {
        self.socket = Some(socket);
    }

    pub(crate) fn poll_jobs(&mut self, workspace_id: crate::workspace::WorkspaceId) {
        if let Some(socket) = self.socket.as_ref() {
            socket.poll_jobs(workspace_id, &mut self.queue);
        }
    }

    #[must_use]
    pub(crate) const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn record_intent(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[must_use]
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.queue.is_empty()
    }

    #[must_use]
    pub(crate) fn pending_count(&self) -> usize {
        self.queue.len()
    }

    #[must_use]
    pub(crate) fn next_job(&self) -> Option<&InboundJob> {
        self.queue.head()
    }

    #[must_use]
    pub(crate) const fn remote_turn_in_flight(&self) -> bool {
        self.started.is_some()
    }

    #[must_use]
    pub(crate) const fn remote_started_at(&self) -> Option<Instant> {
        self.started
    }

    #[must_use]
    pub(crate) const fn receiver_panel_is_warm(&self) -> bool {
        self.receiver_response_id.is_some() && self.started.is_none()
    }

    #[must_use]
    pub(crate) fn interactive_response_id(&self) -> Option<&str> {
        self.interactive_response_id.as_deref()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn interactive_agent_session_id(&self) -> Option<&str> {
        self.interactive_agent_session_id.as_deref()
    }

    #[must_use]
    pub(crate) const fn sync_gate_is_armed(&self) -> bool {
        self.sync_gate.is_some()
    }

    #[cfg(test)]
    pub(crate) fn enqueue(&mut self, job: InboundJob) -> Result<(), StageError> {
        let staged = self.queue.stage(job)?;
        let finalized = self.queue.finalize(staged);
        debug_assert!(finalized, "a runtime finalizes its own staged admission");
        Ok(())
    }

    pub(crate) fn request_receiver_launch(&mut self, actor: ActorContext) {
        self.requested_actor = Some(actor);
    }

    pub(crate) fn cancel_receiver_launch(&mut self) {
        self.requested_actor = None;
    }

    #[cfg(test)]
    pub(crate) fn record_receiver_session(&mut self, response_id: String) {
        self.receiver_response_id = Some(response_id);
    }

    #[must_use]
    pub(crate) fn finish_dispatch(
        &mut self,
        launched: bool,
        job: &InboundJob,
        dispatched_at: Instant,
    ) -> bool {
        let committed = self.queue.commit_head(launched).is_some();
        if !launched {
            self.cancel_receiver_launch();
            self.retry_at = Some(dispatched_at + Duration::from_secs(5));
            return false;
        }
        self.retry_at = None;
        self.sender
            .clone_from(&Some(job.authenticated_sender.clone()));
        self.recipients.clone_from(&job.allowed_response_recipients);
        self.response_email.clone_from(&job.response_email);
        self.email_reply.clone_from(&job.email_reply);
        self.generation = self.generation.saturating_add(1);
        self.started = Some(dispatched_at);
        self.delay_sent = false;
        self.probe = crate::tui::receiver_state::next_probe(0, dispatched_at).map(|due| (due, 0));
        self.renew_lease(job.channel, dispatched_at);
        committed
    }

    #[must_use]
    pub(crate) fn active_remote_turn(&self) -> Option<ActiveRemoteTurn<'_>> {
        Some(ActiveRemoteTurn {
            response_id: self.receiver_response_id.as_deref()?,
            channel: self.lease?.channel,
            sender: self.sender.as_deref()?,
        })
    }

    pub(crate) fn finish_remote_response(&mut self, now: Instant) {
        let Some(channel) = self.active_channel() else {
            return;
        };
        self.clear_delivery_turn();
        self.generation = self.generation.saturating_add(1);
        self.renew_lease(channel, now);
    }

    #[must_use]
    pub(crate) fn active_channel(&self) -> Option<Channel> {
        self.lease.map(|lease| lease.channel)
    }

    #[must_use]
    pub(crate) fn receiver_response_id(&self) -> Option<&str> {
        self.receiver_response_id.as_deref()
    }

    #[must_use]
    pub(crate) fn has_receiver_session(&self) -> bool {
        self.receiver_response_id.is_some()
    }

    #[must_use]
    pub(crate) fn active_delivery_target(&self) -> Option<DeliveryTarget> {
        Some(DeliveryTarget {
            channel: self.active_channel()?,
            sender: self.sender.clone()?,
        })
    }

    #[must_use]
    pub(crate) fn email_reply_target(&self) -> EmailReplyTarget {
        EmailReplyTarget {
            response_email: self.response_email.clone(),
            recipients: self.recipients.clone(),
            reply: self.email_reply.clone(),
        }
    }

    #[must_use]
    pub(crate) fn claim_processing_delay(&mut self, now: Instant) -> Option<DeliveryTarget> {
        if self.delay_sent
            || self.started.is_none_or(|started| {
                now.saturating_duration_since(started) < Duration::from_secs(120)
            })
        {
            return None;
        }
        let target = self.active_delivery_target()?;
        self.delay_sent = true;
        Some(target)
    }

    pub(crate) fn clear_receiver_panel_state(&mut self) {
        self.clear_delivery_turn();
        self.receiver_response_id = None;
        self.lease = None;
        self.requested_actor = None;
    }

    #[must_use]
    pub(crate) fn warm_lease_expired(&self, now: Instant) -> Option<Channel> {
        let lease = self.lease?;
        (lease.deadline <= now
            && self.active_channel() == Some(lease.channel)
            && self.generation == lease.generation
            && self.receiver_panel_is_warm())
        .then_some(lease.channel)
    }

    pub(crate) fn prepare_channel_launch(&mut self, channel: Channel) {
        self.force_fresh = self.new_session_channels.remove(&channel);
    }

    #[must_use]
    pub(crate) fn take_restart(
        &mut self,
    ) -> Option<crate::server::receiver::RestartPlan<InboundJob>> {
        self.queue.take_restart()
    }

    #[must_use]
    pub(crate) fn take_new_session(&mut self) -> Option<InboundJob> {
        let job = self.queue.take_new_session()?;
        self.new_session_channels.insert(job.channel);
        Some(job)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn has_pending_channel_reset(&self) -> bool {
        !self.new_session_channels.is_empty() || self.force_fresh
    }

    fn renew_lease(&mut self, channel: Channel, now: Instant) {
        self.lease = Some(Lease {
            channel,
            generation: self.generation,
            deadline: now + INACTIVITY_LEASE,
        });
    }

    fn clear_delivery_turn(&mut self) {
        self.sender = None;
        self.recipients.clear();
        self.response_email = None;
        self.email_reply = None;
        self.started = None;
        self.delay_sent = false;
        self.probe = None;
        self.panel_activity = None;
    }
}
