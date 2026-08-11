//! Turning one received email into an agent prompt.

/// The most inbound email text brain will hand an agent.
///
/// The prompt is typed into the brain panel's PTY, so its size is not free:
/// the receiving API caps a fetched email at 1 MiB, and an HTML newsletter
/// reaches that ceiling easily. 16 KiB is far more than any message a person
/// writes and small enough to type safely.
pub(super) const MAX_PROMPT_BYTES: usize = 16 * 1024;

const TRUNCATION_NOTICE: &str = "\n\n[truncated by brain: the rest of this email was not included]";

/// Reduce an HTML mail part to the text a person would read.
///
/// Mail sent from a rich client is frequently HTML-only. Handing the agent
/// raw markup wastes the prompt on tags and buries the actual message, so
/// element content is kept, block boundaries become line breaks, and
/// `script`/`style` bodies are dropped entirely rather than read as prose.
pub(super) fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let after = &rest[open..];
        let Some(close) = after.find('>') else {
            text.push_str(after);
            rest = "";
            break;
        };
        let tag = &after[1..close];
        rest = &after[close + 1..];
        if let Some(skipped) = skip_raw_element(tag, rest) {
            rest = skipped;
            continue;
        }
        if breaks_line(tag) {
            text.push('\n');
        }
    }
    text.push_str(rest);
    collapse(&decode_entities(&text))
}

/// Keep a prompt within `limit` bytes, telling the agent when it was cut.
///
/// A silently shortened message would be answered as if it were complete.
pub(super) fn bounded_prompt(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATION_NOTICE}", &text[..end])
}

/// The remainder after an element whose content is code, not prose.
fn skip_raw_element<'a>(tag: &str, rest: &'a str) -> Option<&'a str> {
    let name = tag_name(tag);
    if !matches!(name.as_str(), "script" | "style") {
        return None;
    }
    let closing = format!("</{name}");
    let start = rest.to_ascii_lowercase().find(&closing)?;
    let after = &rest[start..];
    after.find('>').map(|close| &after[close + 1..])
}

fn tag_name(tag: &str) -> String {
    tag.trim_start_matches('/')
        .split(|ch: char| ch.is_whitespace() || ch == '/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn breaks_line(tag: &str) -> bool {
    matches!(
        tag_name(tag).as_str(),
        "br" | "p"
            | "div"
            | "tr"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "table"
    )
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after[..after.len().min(12)].find(';') else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        match &after[1..end] {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" | "#39" => out.push('\''),
            "nbsp" | "#160" => out.push(' '),
            _ => {
                out.push_str(&after[..=end]);
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Trim each line and cap consecutive blank lines at one, so the paragraph
/// structure survives without the whitespace a layout table leaves behind.
fn collapse(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && lines.last().is_some_and(|last| last.is_empty()) {
            continue;
        }
        lines.push(trimmed);
    }
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
