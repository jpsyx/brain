use anyhow::{Context as _, Result};

use crate::state::{
    ReceiverDeliveryId, ReceiverJobId, ReceiverJobToken, ReceiverResponseKind,
    render_receiver_delivery,
};

pub(in crate::state::receiver) fn insert(
    connection: &rusqlite::Connection,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    inbound: &crate::server::receiver::InboundJob,
    kind: ReceiverResponseKind,
    content: &str,
    observed_at_unix_ms: i64,
) -> Result<bool> {
    let envelope = render_receiver_delivery(inbound, kind, &inbound.response_sender, content)?;
    let envelope_json =
        serde_json::to_string(&envelope).context("serialize durable receiver response envelope")?;
    Ok(connection.execute(
        "INSERT OR IGNORE INTO receiver_deliveries
           (delivery_id, job_id, job_token, response_kind, envelope_json,
            state, attempt_count, created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, 'ready', 0, ?6, ?6)",
        rusqlite::params![
            ReceiverDeliveryId::new().to_string(),
            job_id.to_string(),
            token.to_string(),
            kind.as_str(),
            envelope_json,
            observed_at_unix_ms,
        ],
    )? == 1)
}

pub(in crate::state::receiver) const fn channel_label(
    channel: crate::server::receiver::Channel,
) -> &'static str {
    match channel {
        crate::server::receiver::Channel::Sms => "sms",
        crate::server::receiver::Channel::Email => "email",
    }
}
