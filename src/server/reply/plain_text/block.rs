//! Line-level markdown removal: headings, lists, quotes, rules, fences, tables.

/// One output line. Fenced-code content is `verbatim`: its span markers are
/// literal characters, not markup.
pub(super) struct PlainLine {
    pub(super) text: String,
    pub(super) verbatim: bool,
}

impl PlainLine {
    fn prose(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            verbatim: false,
        }
    }
}

/// Strip every block-level marker, dropping lines that carried no content.
pub(super) fn to_plain_lines(text: &str) -> Vec<PlainLine> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for raw in text.lines() {
        let trimmed = raw.trim();
        if is_fence(trimmed) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push(PlainLine {
                text: raw.trim_end().to_owned(),
                verbatim: true,
            });
            continue;
        }
        if is_thematic_break(trimmed) {
            continue;
        }
        let line = strip_quote_markers(trimmed);
        if line.is_empty() {
            out.push(PlainLine::prose(String::new()));
            continue;
        }
        if line.starts_with('|') {
            out.extend(table_cells(&line).map(PlainLine::prose));
            continue;
        }
        if let Some(heading) = heading_text(&line) {
            out.push(PlainLine::prose(heading));
            continue;
        }
        if let Some(item) = bullet_text(&line) {
            out.push(PlainLine::prose(format!("- {item}")));
            continue;
        }
        out.push(PlainLine::prose(line));
    }
    out
}

fn is_fence(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Three or more of one rule character, which also covers a setext underline.
fn is_thematic_break(trimmed: &str) -> bool {
    let marks = trimmed
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    marks.len() >= 3
        && marks
            .first()
            .is_some_and(|first| matches!(first, '-' | '*' | '_' | '='))
        && marks.iter().all(|character| Some(character) == marks.first())
}

fn strip_quote_markers(trimmed: &str) -> String {
    let mut line = trimmed;
    while let Some(rest) = line.strip_prefix('>') {
        line = rest.trim_start();
    }
    line.trim().to_owned()
}

fn heading_text(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|character| *character == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = line.get(hashes..)?;
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim_end().to_owned())
}

/// Any bullet flavour, at any nesting depth, becomes one flat `- ` item: the
/// indentation a phone cannot show is pure character cost.
fn bullet_text(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))?;
    Some(rest.trim().to_owned())
}

/// A table row becomes comma-separated cells; the alignment row is dropped.
fn table_cells(line: &str) -> Option<String> {
    let cells = line
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    let alignment = cells.iter().all(|cell| {
        !cell.is_empty() && cell.chars().all(|character| matches!(character, '-' | ':'))
    });
    if alignment {
        return None;
    }
    Some(
        cells
            .into_iter()
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Vec<String> {
        to_plain_lines(text)
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    #[test]
    fn fenced_content_is_marked_verbatim_and_the_fences_are_dropped() {
        let lines = to_plain_lines("```\na_b_c\n```");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].verbatim);
        assert_eq!(lines[0].text, "a_b_c");
    }

    #[test]
    fn a_nested_quote_loses_every_marker() {
        assert_eq!(plain("> > deep"), vec!["deep"]);
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert_eq!(plain("#1 priority"), vec!["#1 priority"]);
        assert_eq!(plain("#######  seven"), vec!["#######  seven"]);
    }

    #[test]
    fn a_bare_dash_line_is_a_rule_but_a_dash_item_is_a_bullet() {
        assert_eq!(plain("- - -"), Vec::<String>::new());
        assert_eq!(plain("- milk"), vec!["- milk"]);
    }
}
