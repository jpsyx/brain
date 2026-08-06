//! Bounded provider-ID reservations and completed-delivery memory.

use anyhow::Result;

pub(super) type ProviderKey = (crate::workspace::WorkspaceId, super::super::Channel, String);

pub(super) static DELIVERIES: std::sync::LazyLock<std::sync::Mutex<ProviderDeliveries>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(ProviderDeliveries::default()));

pub(in crate::server) fn provider_delivery_completed(
    workspace_id: crate::workspace::WorkspaceId,
    channel: super::super::Channel,
    provider_id: &str,
) -> bool {
    DELIVERIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .completed(&(workspace_id, channel, provider_id.to_owned()))
}

pub(in crate::server) fn remember_verified_unavailable_email(
    workspace_id: crate::workspace::WorkspaceId,
    provider_id: String,
) {
    let key = (workspace_id, super::super::Channel::Email, provider_id);
    let mut deliveries = DELIVERIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if deliveries.begin(key.clone()).started() {
        deliveries.finish(&key, false);
    }
}

pub(super) fn forward_provider_delivery(
    deliveries: &std::sync::Mutex<ProviderDeliveries>,
    key: &ProviderKey,
    forward: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let reservation = deliveries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .begin(key.clone());
    match reservation {
        ProviderReservation::Duplicate => return Ok(()),
        ProviderReservation::InFlight => {
            anyhow::bail!("provider delivery is already being accepted")
        }
        ProviderReservation::Started => {}
    }
    let result = forward();
    deliveries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish(key, result.is_ok());
    result
}

#[derive(Default)]
pub(super) struct ProviderDeliveries {
    pending: std::collections::HashSet<ProviderKey>,
    order: std::collections::VecDeque<ProviderKey>,
    accepted: std::collections::HashSet<ProviderKey>,
}

impl ProviderDeliveries {
    fn completed(&self, key: &ProviderKey) -> bool {
        self.accepted.contains(key)
    }

    pub(super) fn begin(&mut self, key: ProviderKey) -> ProviderReservation {
        if self.accepted.contains(&key) {
            return ProviderReservation::Duplicate;
        }
        if !self.pending.insert(key) {
            return ProviderReservation::InFlight;
        }
        ProviderReservation::Started
    }

    pub(super) fn finish(&mut self, key: &ProviderKey, accepted: bool) {
        const RECENT_PROVIDER_IDS: usize = 1024;
        self.pending.remove(key);
        if (!accepted && key.1 != super::super::Channel::Email)
            || !self.accepted.insert(key.clone())
        {
            return;
        }
        self.order.push_back(key.clone());
        while self.order.len() > RECENT_PROVIDER_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.accepted.remove(&expired);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderReservation {
    Started,
    Duplicate,
    InFlight,
}

impl ProviderReservation {
    pub(super) const fn started(self) -> bool {
        matches!(self, Self::Started)
    }
}
