//! Deterministic append-only Markdown for portable receiver conversations.

use std::fmt::Write as _;

/// Maximum authorized assistant answer retained from one completion artifact.
pub const MAX_RECEIVER_ANSWER_BYTES: usize = 256 * 1024;

/// Append one authenticated inbound turn and its exact assistant answer.
#[must_use]
pub fn render_receiver_transcript(prior: &str, inbound: &str, answer: &str) -> String {
    let turn = render_turn(inbound, answer);
    let mut transcript = String::with_capacity(prior.len() + separator_len(prior) + turn.len());
    transcript.push_str(prior);
    append_separator(&mut transcript);
    transcript.push_str(&turn);
    transcript
}

/// Whether one transcript already ends with this byte-exact rendered turn.
#[must_use]
pub fn receiver_transcript_has_exact_turn(transcript: &str, inbound: &str, answer: &str) -> bool {
    transcript.ends_with(&render_turn(inbound, answer))
}

fn render_turn(inbound: &str, answer: &str) -> String {
    let mut turn = String::with_capacity(inbound.len() + answer.len() + 96);
    turn.push_str("## Authenticated user\n\n");
    append_fenced_text(&mut turn, inbound);
    turn.push_str("\n\n## Assistant\n\n");
    append_fenced_text(&mut turn, answer);
    turn
}

fn append_fenced_text(output: &mut String, content: &str) {
    let fence = "`".repeat(longest_backtick_run(content).saturating_add(1).max(3));
    let _ = writeln!(output, "{fence}text");
    output.push_str(content);
    if !content.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&fence);
}

fn longest_backtick_run(value: &str) -> usize {
    value
        .bytes()
        .fold((0, 0), |(longest, current), byte| {
            if byte == b'`' {
                let current = current + 1;
                (longest.max(current), current)
            } else {
                (longest, 0)
            }
        })
        .0
}

fn separator_len(prior: &str) -> usize {
    if prior.is_empty() || prior.ends_with("\n\n") {
        0
    } else if prior.ends_with('\n') {
        1
    } else {
        2
    }
}

fn append_separator(transcript: &mut String) {
    match separator_len(transcript) {
        0 => {}
        1 => transcript.push('\n'),
        2 => transcript.push_str("\n\n"),
        _ => unreachable!("receiver transcript separator is bounded"),
    }
}
