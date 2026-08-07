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
    pub(crate) fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Construct the sequence for literal typed text.
    #[must_use]
    pub(crate) fn text(text: &str) -> Self {
        let mut bytes = Vec::with_capacity(text.len());
        for character in text.chars() {
            if character == '\n' {
                bytes.extend_from_slice(&[0x1b, b'\r']);
            } else {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
        Self(bytes)
    }

    /// Encode literal text followed by one frontend-defined semantic key sequence.
    #[must_use]
    pub(crate) fn text_with_suffix(text: &str, suffix: &[u8]) -> Self {
        let mut input = Self::text(text).0;
        input.extend_from_slice(suffix);
        Self(input)
    }

    /// Consume the sequence for delivery by a transport implementation.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}
