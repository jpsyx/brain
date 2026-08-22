//! Span-level markdown removal: code, links, emphasis, escapes.

/// Reduce one already-de-scaffolded line to the characters a reader needs.
pub(super) fn strip_spans(line: &str) -> String {
    code_segments(line)
        .into_iter()
        .map(|segment| match segment {
            Segment::Code(code) => code,
            Segment::Prose(prose) => {
                let text = strip_bracket_spans(&prose);
                let text = strip_autolinks(&text);
                let text = strip_emphasis(&text);
                unescape(&text)
            }
        })
        .collect()
}

/// Code content is data, not markup: `` `a*b*c` `` must reach the phone whole.
enum Segment {
    Code(String),
    Prose(String),
}

/// Unwrap `` `code` `` into a protected segment; an unpaired run stays literal.
fn code_segments(text: &str) -> Vec<Segment> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut prose = String::new();
    let mut index = 0;
    while index < chars.len() {
        let run = backtick_run(&chars, index);
        let close = (run > 0)
            .then(|| closing_backtick_run(&chars, index + run, run))
            .flatten();
        match close {
            Some(close) => {
                out.push(Segment::Prose(std::mem::take(&mut prose)));
                out.push(Segment::Code(
                    chars
                        .get(index + run..close)
                        .unwrap_or_default()
                        .iter()
                        .collect(),
                ));
                index = close + run;
            }
            None if run > 0 => {
                prose.extend(std::iter::repeat_n('`', run));
                index += run;
            }
            None => {
                prose.push(chars[index]);
                index += 1;
            }
        }
    }
    out.push(Segment::Prose(prose));
    out
}

fn backtick_run(chars: &[char], from: usize) -> usize {
    chars
        .iter()
        .skip(from)
        .take_while(|character| **character == '`')
        .count()
}

fn closing_backtick_run(chars: &[char], from: usize, width: usize) -> Option<usize> {
    let mut index = from;
    while index < chars.len() {
        let run = backtick_run(chars, index);
        if run == width {
            return Some(index);
        }
        index += if run == 0 { 1 } else { run };
    }
    None
}

/// `![alt](url)` keeps the alt text; a link keeps its label *and* a reachable
/// target, because a phone reader cannot follow a label. A local or relative
/// target is dropped: it is unreachable from the phone, so it is only noise.
fn strip_bracket_spans(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '[' || escaped(&chars, index) {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let Some(span) = bracket_span(&chars, index) else {
            out.push(chars[index]);
            index += 1;
            continue;
        };
        let label = span_text(&chars, index + 1, span.label_end);
        let image = index > 0 && chars[index - 1] == '!';
        if image {
            out.pop();
            out.push_str(&label);
        } else {
            out.push_str(&link_text(
                &label,
                &span_text(&chars, span.label_end + 2, span.end),
            ));
        }
        index = span.end + 1;
    }
    out
}

struct BracketSpan {
    label_end: usize,
    end: usize,
}

fn span_text(chars: &[char], from: usize, to: usize) -> String {
    chars.get(from..to).unwrap_or_default().iter().collect()
}

fn link_text(label: &str, target: &str) -> String {
    let target = target.split_whitespace().next().unwrap_or_default();
    let label = label.trim();
    if !is_address(target) || label == target {
        return if label.is_empty() {
            target.to_owned()
        } else {
            label.to_owned()
        };
    }
    if label.is_empty() {
        target.to_owned()
    } else {
        format!("{label} ({target})")
    }
}

/// Locate `]` then a directly following `(`…`)`, or report no link at all.
fn bracket_span(chars: &[char], open: usize) -> Option<BracketSpan> {
    let label_end =
        (open + 1..chars.len()).find(|index| chars[*index] == ']' && !escaped(chars, *index))?;
    if chars.get(label_end + 1) != Some(&'(') {
        return None;
    }
    let end = (label_end + 2..chars.len())
        .find(|index| chars[*index] == ')' && !escaped(chars, *index))?;
    Some(BracketSpan { label_end, end })
}

/// `<https://example.test>` and `<someone@example.test>` keep the address.
fn strip_autolinks(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        let inner = (chars[index] == '<' && !escaped(&chars, index))
            .then(|| (index + 1..chars.len()).find(|at| chars[*at] == '>'))
            .flatten()
            .map(|close| {
                (
                    close,
                    chars
                        .get(index + 1..close)
                        .unwrap_or_default()
                        .iter()
                        .collect::<String>(),
                )
            })
            .filter(|(_, inner)| is_address(inner));
        if let Some((close, inner)) = inner {
            out.push_str(&inner);
            index = close + 1;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn is_address(inner: &str) -> bool {
    !inner.chars().any(char::is_whitespace)
        && (inner.contains("://") || inner.starts_with("mailto:") || inner.contains('@'))
}

/// Emphasis markers are removed widest-first so `**bold**` never degrades into
/// a stray `*bold*`.
fn strip_emphasis(text: &str) -> String {
    const PASSES: [(char, usize, bool); 5] = [
        ('~', 2, true),
        ('*', 2, true),
        ('_', 2, false),
        ('*', 1, true),
        ('_', 1, false),
    ];
    PASSES
        .into_iter()
        .fold(text.to_owned(), |current, (marker, width, intraword)| {
            strip_pairs(&current, marker, width, intraword)
        })
}

fn strip_pairs(text: &str, marker: char, width: usize, intraword: bool) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if !marker_at(&chars, index, marker, width) || escaped(&chars, index) {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let closer = opens_emphasis(&chars, index, marker, width, intraword)
            .then(|| closing_marker(&chars, index + width, marker, width, intraword))
            .flatten();
        if let Some(close) = closer {
            out.extend(chars.get(index + width..close).unwrap_or_default());
            index = close + width;
        } else {
            out.extend(std::iter::repeat_n(marker, width));
            index += width;
        }
    }
    out
}

fn marker_at(chars: &[char], at: usize, marker: char, width: usize) -> bool {
    chars
        .get(at..at + width)
        .is_some_and(|run| run.iter().all(|character| *character == marker))
}

/// An opener is glued to its content, and `_` additionally may not start
/// inside a word so `snake_case_name` survives.
fn opens_emphasis(chars: &[char], at: usize, marker: char, width: usize, intraword: bool) -> bool {
    chars
        .get(at + width)
        .is_some_and(|next| !next.is_whitespace() && *next != marker)
        && (intraword || at == 0 || !chars[at - 1].is_alphanumeric())
}

fn closing_marker(
    chars: &[char],
    from: usize,
    marker: char,
    width: usize,
    intraword: bool,
) -> Option<usize> {
    (from + 1..chars.len()).find(|at| {
        marker_at(chars, *at, marker, width)
            && !escaped(chars, *at)
            && chars
                .get(at - 1)
                .is_some_and(|previous| !previous.is_whitespace())
            && (intraword
                || chars
                    .get(at + width)
                    .is_none_or(|next| !next.is_alphanumeric()))
    })
}

fn escaped(chars: &[char], at: usize) -> bool {
    at > 0 && chars[at - 1] == '\\'
}

/// A backslash before punctuation only existed to hide a marker.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            continue;
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unpaired_marker_is_left_as_written() {
        assert_eq!(strip_spans("a ` b"), "a ` b");
        assert_eq!(strip_spans("**unclosed bold"), "**unclosed bold");
        assert_eq!(strip_spans("[not a link] here"), "[not a link] here");
    }

    #[test]
    fn a_code_span_wins_over_the_markers_inside_it() {
        assert_eq!(strip_spans("use `a*b*c` here"), "use a*b*c here");
    }

    #[test]
    fn an_angle_bracket_that_is_not_an_address_stays_literal() {
        assert_eq!(strip_spans("3 <4 and 5> 2"), "3 <4 and 5> 2");
        assert_eq!(strip_spans("<not a url>"), "<not a url>");
    }
}
