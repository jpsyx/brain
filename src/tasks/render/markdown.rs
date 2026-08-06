//! The small markdown subset shared by expanded task notes and the Brain
//! "finished working" panel: inline `**bold**` / `*italic*` / `` `code` ``
//! plus block-level headings, bullets, and blockquotes.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tasks::task::Task;

use super::style::{ACCENT_CYAN, TEXT_DIM, TEXT_PRIMARY, accent, truncate, very_dim};

const NOTE_CODE: Color = super::style::ACCENT_GREEN; // inline `code` spans

/// Index of the next single `target` char at or after `from`.
fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&j| chars[j] == target)
}

/// Index of the next `**` (two-char marker) at or after `from`.
fn find_double_star(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&j| chars[j] == '*' && chars[j + 1] == '*')
}

/// Turn one line of note text into styled spans, honoring inline
/// `**bold**`, `*italic*` / `_italic_`, and `` `code` `` markers. Unclosed
/// markers are emitted as literal text. `base` styles the plain runs;
/// emphasized runs layer modifiers / colors on top of it.
fn inline_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    let flush = |spans: &mut Vec<Span<'static>>, buf: &mut String| {
        if !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(buf), base));
        }
    };

    while i < chars.len() {
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(close) = find_double_star(&chars, i + 2) {
                flush(&mut spans, &mut buf);
                let content: String = chars[i + 2..close].iter().collect();
                spans.push(Span::styled(content, base.add_modifier(Modifier::BOLD)));
                i = close + 2;
                continue;
            }
        }
        if chars[i] == '`' {
            if let Some(close) = find_char(&chars, i + 1, '`') {
                flush(&mut spans, &mut buf);
                let content: String = chars[i + 1..close].iter().collect();
                spans.push(Span::styled(content, Style::new().fg(NOTE_CODE)));
                i = close + 1;
                continue;
            }
        }
        if chars[i] == '*' || chars[i] == '_' {
            let marker = chars[i];
            if let Some(close) = find_char(&chars, i + 1, marker) {
                if close > i + 1 {
                    flush(&mut spans, &mut buf);
                    let content: String = chars[i + 1..close].iter().collect();
                    spans.push(Span::styled(content, base.add_modifier(Modifier::ITALIC)));
                    i = close + 1;
                    continue;
                }
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut spans, &mut buf);
    spans
}

/// Strip a leading ATX heading marker (`#`..`######` + space); the heading
/// text is returned when present.
fn heading_body(s: &str) -> Option<&str> {
    let hashes = s.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        return s[hashes..].strip_prefix(' ');
    }
    None
}

/// Strip a leading unordered-list marker (`-`, `*`, or `+` followed by a
/// space); the item text is returned when present.
fn bullet_body(s: &str) -> Option<&str> {
    ['-', '*', '+']
        .into_iter()
        .find_map(|m| s.strip_prefix(m).and_then(|rest| rest.strip_prefix(' ')))
}

/// Render expanded notes as a small markdown subset: one source line maps
/// to one rendered `Line`, newlines are preserved, leading indentation is
/// kept (nested lists), and headings / bullets / blockquotes get block
/// styling. Every line carries the priority accent gutter so the card reads
/// as one unit; the `↳` glyph marks only the first line.
fn markdown_note_lines(notes: &str, priority: &str) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for (idx, raw) in notes.split('\n').enumerate() {
        let line = raw.trim_end();
        let glyph = if idx == 0 { "↳ " } else { "  " };
        let mut spans: Vec<Span<'static>> = vec![accent(priority), very_dim(glyph)];

        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            out.push(Line::from(spans));
            continue;
        }
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        if indent > 0 {
            spans.push(Span::raw(" ".repeat(indent)));
        }

        if let Some(body) = heading_body(trimmed) {
            spans.extend(inline_spans(
                body,
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            ));
        } else if let Some(body) = bullet_body(trimmed) {
            spans.push(Span::styled("• ", Style::new().fg(ACCENT_CYAN)));
            spans.extend(inline_spans(body, Style::new().fg(TEXT_DIM)));
        } else if let Some(body) = trimmed.strip_prefix("> ") {
            spans.push(very_dim("▏ "));
            spans.extend(inline_spans(body, Style::new().fg(TEXT_DIM)));
        } else {
            spans.extend(inline_spans(trimmed, Style::new().fg(TEXT_DIM)));
        }
        out.push(Line::from(spans));
    }
    out
}

/// Notes block for a task. Empty notes render nothing. When `expanded`,
/// the full notes are rendered as multi-line markdown; otherwise a single
/// truncated preview line (newlines flattened) is shown.
pub(super) fn notes_lines(task: &Task, expanded: bool) -> Vec<Line<'static>> {
    let trimmed = task.notes.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if expanded {
        return markdown_note_lines(task.notes.trim_matches('\n'), &task.priority);
    }
    vec![Line::from(vec![
        accent(&task.priority),
        very_dim("↳ "),
        Span::styled(truncate(trimmed, 110), Style::new().fg(TEXT_DIM)),
    ])]
}

#[cfg(test)]
mod tests {
    use super::{
        bullet_body, heading_body, inline_spans, markdown_note_lines, notes_lines,
    };
    use crate::tasks::task::test_task;
    use ratatui::style::{Modifier, Style};

    // --- inline markdown ---

    /// Concatenate the rendered span contents so tests can assert on the
    /// flat text without caring about span boundaries.
    fn flat(spans: &[ratatui::text::Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn inline_bold_splits_into_a_bold_span() {
        let spans = inline_spans("a **b** c", Style::new());
        assert_eq!(flat(&spans), "a b c");
        let bold = spans
            .iter()
            .find(|s| s.content == "b")
            .expect("bold run present");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_italic_handles_both_markers() {
        for src in ["an *x* y", "an _x_ y"] {
            let spans = inline_spans(src, Style::new());
            assert_eq!(flat(&spans), "an x y");
            let it = spans.iter().find(|s| s.content == "x").unwrap();
            assert!(it.style.add_modifier.contains(Modifier::ITALIC));
        }
    }

    #[test]
    fn inline_code_strips_backticks_and_recolors() {
        let spans = inline_spans("run `cargo test`", Style::new());
        assert_eq!(flat(&spans), "run cargo test");
        assert!(spans.iter().any(|s| s.content == "cargo test"));
    }

    #[test]
    fn inline_unclosed_marker_is_literal() {
        let spans = inline_spans("a * b", Style::new());
        assert_eq!(flat(&spans), "a * b");
        assert!(spans.iter().all(|s| !s.style.add_modifier.contains(Modifier::ITALIC)));
    }

    // --- block markdown ---

    #[test]
    fn heading_body_strips_hashes_and_requires_space() {
        assert_eq!(heading_body("## Title"), Some("Title"));
        assert_eq!(heading_body("# H"), Some("H"));
        assert_eq!(heading_body("#NoSpace"), None);
        assert_eq!(heading_body("plain"), None);
        assert_eq!(heading_body("####### too many"), None);
    }

    #[test]
    fn bullet_body_recognizes_dash_star_plus() {
        assert_eq!(bullet_body("- item"), Some("item"));
        assert_eq!(bullet_body("* item"), Some("item"));
        assert_eq!(bullet_body("+ item"), Some("item"));
        // No space → not a bullet (so `*italic*` lines are left alone).
        assert_eq!(bullet_body("*italic*"), None);
    }

    #[test]
    fn markdown_note_lines_preserves_newlines_one_line_each() {
        let lines = markdown_note_lines("first\nsecond\nthird", "p2");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn markdown_note_lines_blank_line_kept() {
        let lines = markdown_note_lines("a\n\nb", "p2");
        assert_eq!(lines.len(), 3);
    }


    // --- notes_lines ---

    #[test]
    fn notes_lines_empty_renders_nothing() {
        let t = test_task("T1", "not_started");
        assert!(notes_lines(&t, false).is_empty());
        assert!(notes_lines(&t, true).is_empty());
    }

    #[test]
    fn notes_lines_collapsed_is_single_truncated_line() {
        let mut t = test_task("T1", "not_started");
        t.notes = "line one\nline two\nline three".to_owned();
        let lines = notes_lines(&t, false);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn notes_lines_expanded_renders_each_source_line() {
        let mut t = test_task("T1", "not_started");
        t.notes = "# Heading\n- a\n- b".to_owned();
        let lines = notes_lines(&t, true);
        assert_eq!(lines.len(), 3);
    }
}
