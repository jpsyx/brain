//! Whether a Claude transcript actually holds a conversation.
//!
//! `<id>.jsonl` existing is not the same as `claude --resume <id>` finding
//! something to resume. Claude writes metadata records — an AI-generated title,
//! an agent name — for a session that was named but never spoken in, which is
//! exactly what a background agent's fork stub looks like. Resuming one answers
//! *"No conversation found with session ID"* and quits.

/// Record types that carry an actual turn. Everything else Claude writes
/// (`ai-title`, `agent-name`, `mode`, `file-history-snapshot`, …) is bookkeeping
/// around a conversation rather than the conversation itself.
const TURN_TYPES: [&str; 2] = ["user", "assistant"];

/// True once any line records a real exchange. Unreadable lines contribute
/// nothing rather than disqualifying the transcript: Claude owns this format
/// and may add records we don't know about.
#[must_use]
pub(crate) fn transcript_has_conversation(contents: &str) -> bool {
    contents.lines().any(line_is_turn)
}

fn line_is_turn(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .as_ref()
        .and_then(|record| record.get("type")?.as_str().map(str::to_owned))
        .is_some_and(|kind| TURN_TYPES.contains(&kind.as_str()))
}
