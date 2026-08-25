/// Durable lifecycle of one accepted receiver job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverJobState {
    Queued,
    Claimed,
    Launching,
    Launched,
    Accepted,
    Processing,
    AnswerReady,
    Delivering,
    Retrying,
    Failed,
    Done,
}

impl ReceiverJobState {
    /// Whether the lifecycle permits this exact next state.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Claimed | Self::Failed)
                | (
                    Self::Claimed,
                    Self::Launching | Self::Retrying | Self::Failed
                )
                | (
                    Self::Launching,
                    Self::Launched | Self::Retrying | Self::Failed
                )
                | (
                    Self::Launched,
                    Self::Accepted | Self::Processing | Self::Done
                )
                | (
                    Self::Accepted,
                    Self::Processing | Self::Retrying | Self::Failed
                )
                | (
                    Self::Processing,
                    Self::AnswerReady | Self::Retrying | Self::Failed
                )
                | (Self::AnswerReady, Self::Delivering | Self::Failed)
                | (Self::Delivering, Self::Done | Self::Retrying | Self::Failed)
                | (
                    Self::Retrying,
                    Self::Claimed | Self::Launching | Self::Delivering | Self::Failed
                )
        )
    }

    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Launching => "launching",
            Self::Launched => "launched",
            Self::Accepted => "accepted",
            Self::Processing => "processing",
            Self::AnswerReady => "answer-ready",
            Self::Delivering => "delivering",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
            Self::Done => "done",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "claimed" => Some(Self::Claimed),
            "launching" => Some(Self::Launching),
            "launched" => Some(Self::Launched),
            "accepted" => Some(Self::Accepted),
            "processing" => Some(Self::Processing),
            "answer-ready" => Some(Self::AnswerReady),
            "delivering" => Some(Self::Delivering),
            "retrying" => Some(Self::Retrying),
            "failed" => Some(Self::Failed),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}
