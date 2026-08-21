//! Live-TUI receiver state with representation-owned queue behavior.

mod queue;

pub use queue::{InboundQueue, StageError, StagedAdmission};
