//! Markdown → plain text for media that cannot render markup.
//!
//! SMS has no renderer, so every `#`, `*`, `[`, and `|` a model writes reaches
//! the phone as literal noise and spends characters brain does not have. The
//! conversion is deliberately a small, pure, line-oriented pass rather than a
//! full markdown parser: it must never fail, never reorder content, and never
//! swallow text it did not recognize.

mod block;
mod inline;

/// Render markdown as the plain text a phone can display.
#[must_use]
pub fn strip_markdown(text: &str) -> String {
    let lines = block::to_plain_lines(text)
        .into_iter()
        .map(|line| {
            if line.verbatim {
                line.text
            } else {
                inline::strip_spans(&line.text)
            }
        })
        .collect::<Vec<_>>();
    collapse_blank_runs(lines)
}

/// One blank line is a paragraph break; more is wasted length.
fn collapse_blank_runs(lines: Vec<String>) -> String {
    let mut kept = Vec::<String>::new();
    for line in lines {
        let line = line.trim_end().to_owned();
        if line.is_empty() && kept.last().is_none_or(String::is_empty) {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_lose_their_markers() {
        assert_eq!(strip_markdown("# Today\n\nTwo tasks"), "Today\n\nTwo tasks");
        assert_eq!(strip_markdown("### Later ###"), "Later");
    }

    #[test]
    fn emphasis_code_and_strikethrough_keep_only_their_content() {
        assert_eq!(
            strip_markdown("**Call** the *vet* about `rabies` and ~~fleas~~"),
            "Call the vet about rabies and fleas"
        );
        assert_eq!(strip_markdown("__Bold__ and _thin_"), "Bold and thin");
    }

    #[test]
    fn a_link_keeps_both_its_label_and_a_reachable_address() {
        assert_eq!(
            strip_markdown("See [the invoice](https://example.test/a) today"),
            "See the invoice (https://example.test/a) today",
            "a label alone leaves the reader nothing to open"
        );
        assert_eq!(
            strip_markdown("See [the note](../areas/money/a.md) today"),
            "See the note today",
            "a local target is not reachable from a phone, so it is only noise"
        );
        assert_eq!(
            strip_markdown("[https://example.test/a](https://example.test/a)"),
            "https://example.test/a",
            "a self-labelled link must not be printed twice"
        );
    }

    #[test]
    fn links_keep_the_label_and_bare_urls_survive() {
        assert_eq!(
            strip_markdown("[](https://example.test/a)"),
            "https://example.test/a"
        );
        assert_eq!(
            strip_markdown("<https://example.test/a>"),
            "https://example.test/a"
        );
        assert_eq!(
            strip_markdown("Open https://example.test/a now"),
            "Open https://example.test/a now"
        );
    }

    #[test]
    fn images_are_reduced_to_their_alt_text() {
        assert_eq!(
            strip_markdown("![a chart](https://x.test/c.png)"),
            "a chart"
        );
        assert_eq!(strip_markdown("![](https://x.test/c.png)"), "");
    }

    #[test]
    fn bullets_become_one_plain_dash_and_numbers_keep_their_order() {
        assert_eq!(
            strip_markdown("* Pay rent\n  - call bank\n+ Book flight"),
            "- Pay rent\n- call bank\n- Book flight"
        );
        assert_eq!(strip_markdown("1. First\n2. Second"), "1. First\n2. Second");
    }

    #[test]
    fn quotes_rules_and_fences_drop_their_scaffolding() {
        assert_eq!(strip_markdown("> quoted line"), "quoted line");
        assert_eq!(strip_markdown("A\n\n---\n\nB"), "A\n\nB");
        assert_eq!(strip_markdown("```rust\nlet x = 1;\n```"), "let x = 1;");
    }

    #[test]
    fn a_table_becomes_readable_comma_separated_rows() {
        assert_eq!(
            strip_markdown("| Task | Due |\n| --- | --- |\n| Rent | Friday |"),
            "Task, Due\nRent, Friday"
        );
    }

    #[test]
    fn escaped_punctuation_loses_the_backslash() {
        assert_eq!(
            strip_markdown(r"5 \* 4 and a \_name\_"),
            "5 * 4 and a _name_"
        );
    }

    #[test]
    fn blank_runs_and_trailing_space_collapse_to_save_characters() {
        assert_eq!(
            strip_markdown("First   \n\n\n\nSecond  "),
            "First\n\nSecond"
        );
    }

    #[test]
    fn asterisks_that_are_not_emphasis_are_left_alone() {
        assert_eq!(strip_markdown("2 * 3 * 4"), "2 * 3 * 4");
        assert_eq!(strip_markdown("snake_case_name"), "snake_case_name");
    }

    #[test]
    fn plain_prose_is_returned_untouched() {
        let prose = "Rent is due Friday. Call the vet at +1 555 010 1234.";
        assert_eq!(strip_markdown(prose), prose);
    }
}
