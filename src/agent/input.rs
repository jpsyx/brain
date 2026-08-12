//! Input sequences passed from a frontend-neutral controller to a transport.

/// Terminal bracketed-paste delimiters (DEC mode 2004).
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// The literal text a bracketed paste carries, inverting [`InputSequence::text`].
///
/// Test doubles stand in for a frontend's paste handling, so they decode the
/// payload the same way a real one does and assert on semantic input rather
/// than wire framing.
#[cfg(test)]
#[must_use]
pub(crate) fn paste_payload(bytes: &[u8]) -> String {
    let payload = bytes
        .strip_prefix(PASTE_START)
        .and_then(|rest| rest.strip_suffix(PASTE_END))
        .unwrap_or(bytes);
    String::from_utf8_lossy(payload).replace('\r', "\n")
}

/// Whether a character must not survive into a paste payload.
///
/// A real paste never carries control bytes, and letting one through would let
/// message text close the paste early and be executed as keystrokes.
fn is_paste_hostile(character: char) -> bool {
    character.is_control() && character != '\t'
}

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
    ///
    /// The text is delivered as one bracketed paste, exactly as a terminal
    /// hands over clipboard content: newlines become the `CR` a real paste
    /// carries, and every frontend inserts the payload literally instead of
    /// interpreting it as keystrokes. That is what makes a multi-line prompt
    /// safe to inject while the composer is in vim mode, where a bare `ESC`
    /// would leave insert mode and turn the rest of the message into
    /// normal-mode commands.
    #[must_use]
    pub(crate) fn text(text: &str) -> Self {
        let mut bytes = Vec::with_capacity(text.len() + PASTE_START.len() + PASTE_END.len());
        bytes.extend_from_slice(PASTE_START);
        for character in text.chars() {
            if character == '\n' {
                bytes.push(b'\r');
            } else if !is_paste_hostile(character) {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
        bytes.extend_from_slice(PASTE_END);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Newlines were once encoded as the `ESC CR` "insert a literal newline"
    /// chord. When the frontend's composer is in vim mode that `ESC` leaves
    /// insert mode, so the remainder of the message is executed as normal-mode
    /// commands and the prompt sits unsubmitted in the composer forever.
    /// Bracketed paste carries the same text with no escape the editor can
    /// interpret as a mode change.
    #[test]
    fn multi_line_text_never_sends_an_escape_a_vim_mode_editor_would_consume() {
        assert_eq!(
            InputSequence::text("first\nsecond").into_bytes(),
            b"\x1b[200~first\rsecond\x1b[201~".to_vec()
        );
    }

    /// Message text arrives from an outside sender, so it must not be able to
    /// close the paste and have the remainder run as keystrokes.
    #[test]
    fn message_text_cannot_close_the_paste_and_inject_keystrokes() {
        let bytes = InputSequence::text("before\x1b[201~\rrm -rf /\rafter").into_bytes();
        assert_eq!(
            bytes,
            b"\x1b[200~before[201~rm -rf /after\x1b[201~".to_vec()
        );
        assert_eq!(
            bytes
                .windows(PASTE_END.len())
                .filter(|window| *window == PASTE_END)
                .count(),
            1,
            "exactly one paste terminator, at the very end"
        );
    }

    /// The submit key is a real keystroke and must land after the paste closes,
    /// never inside it where it would be inserted as literal text.
    #[test]
    fn a_semantic_suffix_follows_the_closed_paste() {
        assert_eq!(
            InputSequence::text_with_suffix("ask", b"\r").into_bytes(),
            b"\x1b[200~ask\x1b[201~\r".to_vec()
        );
    }
}
