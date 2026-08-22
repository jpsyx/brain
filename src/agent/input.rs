//! Input sequences passed from a frontend-neutral controller to a transport.

use std::time::Duration;

/// Terminal bracketed-paste delimiters (DEC mode 2004).
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// How long a frontend is given to apply a pasted prompt before the key that
/// submits it arrives.
///
/// A terminal frontend handles a paste and a keystroke on different paths: the
/// paste is buffered and applied to the composer, while the key is dispatched
/// straight to the focused handler. Delivered in one write, the key can be
/// handled against a composer the paste has not reached yet, which submits
/// nothing and leaves the prompt sitting in the composer with no turn behind
/// it. Measured against a real frontend: sharing the write loses the submit,
/// and a separate write a few hundred milliseconds later always lands. Sized
/// with margin, and paid only on an injected follow-up, never on typing.
pub(crate) const PASTE_SETTLE: Duration = Duration::from_millis(400);

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

/// One write a transport performs on the frontend's behalf.
///
/// `settle` is how long the frontend must be left alone *before* these bytes
/// are written, so a write can wait for the effect of the one before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputWrite {
    pub settle: Duration,
    pub bytes: Vec<u8>,
}

/// Bytes a transport should deliver as one agent input sequence.
///
/// Callers use [`crate::agent::AgentController`] semantic methods rather than
/// constructing frontend keystrokes directly. Frontends create the terminal
/// sequence for a semantic submit, queue, or new-session operation. A sequence
/// is a list of writes rather than one buffer because some frontend input is
/// only handled correctly when it arrives separated in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSequence(Vec<InputWrite>);

impl InputSequence {
    /// Construct a frontend-defined input sequence delivered in one write.
    #[must_use]
    pub(crate) fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(vec![InputWrite {
            settle: Duration::ZERO,
            bytes: bytes.into(),
        }])
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
        Self::bytes(bytes)
    }

    /// Encode literal text, then one frontend-defined semantic key that acts on
    /// it, as two writes separated by [`PASTE_SETTLE`].
    ///
    /// The key is what turns typed text into a submitted prompt, so it must be
    /// handled against a composer that already holds the text.
    #[must_use]
    pub(crate) fn text_then_key(text: &str, key: &[u8]) -> Self {
        let mut writes = Self::text(text).0;
        writes.push(InputWrite {
            settle: PASTE_SETTLE,
            bytes: key.to_vec(),
        });
        Self(writes)
    }

    /// Consume the sequence for delivery by a transport implementation.
    #[must_use]
    pub fn into_writes(self) -> Vec<InputWrite> {
        self.0
    }

    /// The writes this sequence is delivered as, in order.
    #[must_use]
    pub fn writes(&self) -> &[InputWrite] {
        &self.0
    }

    /// Every byte the sequence carries, ignoring how it is paced.
    ///
    /// Diagnostics and test doubles care what was said, not how it was timed.
    #[must_use]
    pub fn flattened(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|write| write.bytes.iter().copied())
            .collect()
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
            InputSequence::text("first\nsecond").flattened(),
            b"\x1b[200~first\rsecond\x1b[201~".to_vec()
        );
    }

    /// Message text arrives from an outside sender, so it must not be able to
    /// close the paste and have the remainder run as keystrokes.
    #[test]
    fn message_text_cannot_close_the_paste_and_inject_keystrokes() {
        let bytes = InputSequence::text("before\x1b[201~\rrm -rf /\rafter").flattened();
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
            InputSequence::text_then_key("ask", b"\r").flattened(),
            b"\x1b[200~ask\x1b[201~\r".to_vec()
        );
    }

    /// A frontend that receives the paste and the submit key in one write can
    /// apply the paste *after* it handles the key, so the key falls on a
    /// composer that is still empty and the prompt is left sitting there
    /// forever. Delivering the key as its own write, after the frontend has
    /// been given time to apply the paste, is what makes the submit stick.
    #[test]
    fn the_submit_key_is_its_own_write_and_waits_for_the_paste_to_land() {
        let writes = InputSequence::text_then_key("ask", b"\r").into_writes();
        assert_eq!(
            writes.len(),
            2,
            "the paste and the submit key must not share a write"
        );
        assert_eq!(writes[0].bytes, b"\x1b[200~ask\x1b[201~".to_vec());
        assert_eq!(
            writes[0].settle,
            Duration::ZERO,
            "the text goes out at once"
        );
        assert_eq!(writes[1].bytes, b"\r".to_vec());
        assert!(
            writes[1].settle >= Duration::from_millis(250),
            "the key must wait long enough for the paste to be applied"
        );
    }

    /// Everything that is not a paste-plus-key is one write with nothing to
    /// wait for; a settle delay on an ordinary keystroke would only add lag.
    #[test]
    fn plain_input_is_delivered_immediately_as_a_single_write() {
        for sequence in [InputSequence::bytes(b"\r"), InputSequence::text("hello")] {
            let writes = sequence.into_writes();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].settle, Duration::ZERO);
        }
    }
}
