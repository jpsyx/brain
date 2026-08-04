//! Input sequences passed from a frontend-neutral controller to a transport.

/// Bytes a transport should deliver as one agent input sequence.
///
/// Callers use [`crate::agent::AgentController`] semantic methods rather than
/// constructing frontend keystrokes directly. Frontends create the terminal
/// sequence for a semantic submit, queue, or new-session operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSequence(Vec<u8>);

impl InputSequence {
    /// Construct a frontend-defined input sequence.
    #[must_use]
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Construct the sequence for literal typed text.
    #[must_use]
    pub(crate) fn text(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }

    /// Prefix a frontend input sequence with semantic text.
    #[must_use]
    pub(crate) fn prefixed_with(self, text: &str) -> Self {
        let mut bytes = Vec::with_capacity(text.len() + self.0.len());
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(&self.0);
        Self(bytes)
    }

    /// Consume the sequence for delivery by a transport implementation.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}
