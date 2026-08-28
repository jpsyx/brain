use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The authenticated external channel that initiated a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    Sms,
    Email,
}

/// One provider-owned attachment reference accepted with an inbound job.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRef {
    pub url: String,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub content_type: Option<String>,
    pub filename: Option<String>,
}

impl std::fmt::Debug for AttachmentRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AttachmentRef(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailReplyContext {
    pub provider_email_id: String,
    pub subject: String,
    pub message_id: Option<String>,
}

impl std::fmt::Debug for EmailReplyContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("EmailReplyContext(<redacted>)")
    }
}

/// Immutable authenticated work accepted by exactly one live workspace TUI.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboundJob {
    #[serde(with = "uuid_string")]
    pub job_id: Uuid,
    pub workspace_id: crate::workspace::WorkspaceId,
    pub actor: crate::actor::ActorContext,
    pub channel: Channel,
    pub authenticated_sender: String,
    #[serde(skip)]
    pub response_sender: String,
    pub prompt: String,
    pub attachments: Vec<AttachmentRef>,
    pub received_at_unix_ms: u64,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub thread_participants: Vec<String>,
    #[serde(default)]
    pub response_email: Option<String>,
    #[serde(default)]
    pub allowed_response_recipients: Vec<String>,
    #[serde(default)]
    pub email_reply: Option<EmailReplyContext>,
}

impl std::fmt::Debug for InboundJob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InboundJob(<redacted>)")
    }
}

mod uuid_string {
    use serde::{Deserialize as _, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(value: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Uuid::parse_str(&value).map_err(serde::de::Error::custom)
    }
}
