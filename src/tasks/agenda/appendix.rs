//! Baking caller-supplied optional content into the agenda.
//!
//! The agenda's core is Brain's; anything else a caller wants on the page goes
//! in one **generic marked section** at the bottom. The marker is what makes a
//! rerun replace rather than duplicate, and what tells the sync where core
//! sections must stop.
//!
//! Core knows nothing about what the content *is*: the caller names both files.
//! The source's own H1 is dropped (it would double up under the wrapper's
//! heading) and its remaining headings are demoted, so the wrapper stays the
//! only `## ` boundary the appendix introduces — which is exactly what keeps
//! [`super::doc`]'s section split honest.

use super::APPENDIX_HEADING;

/// Drop a leading `# Title` line, and one blank line after it.
fn strip_leading_h1(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    let Some(first) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return text.to_owned();
    };
    if !lines[first].starts_with("# ") {
        return text.to_owned();
    }
    lines.remove(first);
    if lines.get(first).is_some_and(|line| line.trim().is_empty()) {
        lines.remove(first);
    }
    lines.join("\n")
}

/// Demote every ATX heading so the shallowest becomes `###`, capped at `######`.
fn demote_headings(text: &str) -> String {
    text.lines()
        .map(|line| {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            let is_heading = (1..=6).contains(&hashes)
                && line.chars().nth(hashes).is_some_and(char::is_whitespace);
            if is_heading {
                let level = (hashes + 1).clamp(3, 6);
                format!("{}{}", "#".repeat(level), &line[hashes..])
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wrap caller content in the generic boundary.
pub(crate) fn assemble(content: &str) -> String {
    let body = demote_headings(&strip_leading_h1(content));
    format!("{APPENDIX_HEADING}\n\n{}\n", body.trim())
}

/// The agenda with any prior marked section removed.
pub(crate) fn strip_existing(agenda: &str) -> String {
    agenda
        .lines()
        .position(|line| line == APPENDIX_HEADING)
        .map_or_else(
            || agenda.trim_end().to_owned(),
            |index| {
                agenda
                    .lines()
                    .take(index)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim_end()
                    .to_owned()
            },
        )
}

/// Replace the marked section with `content`.
pub(crate) fn bake(agenda: &str, content: &str) -> String {
    format!("{}\n\n{}", strip_existing(agenda), assemble(content))
}

#[cfg(test)]
mod tests {
    use super::{assemble, bake, strip_existing};

    #[test]
    fn the_sources_own_title_is_dropped() {
        let out = assemble("# Email triage\n\nBody text\n");
        assert!(!out.contains("# Email triage"), "{out}");
        assert!(out.contains("Body text"), "{out}");
    }

    #[test]
    fn headings_are_demoted_below_the_wrapper() {
        let out = assemble("## Section\n\n### Sub\n\n###### Deep\n");
        // The wrapper must stay the only `## ` line, or the agenda's section
        // split would treat the appendix body as core sections.
        assert_eq!(out.matches("\n## ").count(), 0, "{out}");
        assert!(out.starts_with("## Appendix <!-- brain:optional-content -->"));
        assert!(out.contains("### Section"), "{out}");
        assert!(out.contains("#### Sub"), "{out}");
        assert!(out.contains("###### Deep"), "capped at six:\n{out}");
    }

    #[test]
    fn a_hash_that_is_not_a_heading_is_left_alone() {
        assert!(assemble("#hashtag not a heading\n").contains("#hashtag"));
    }

    #[test]
    fn baking_twice_replaces_rather_than_duplicates() {
        let agenda = "# Monday\n\n## Suggested order\n\n1. **T1** Ship\n";
        let once = bake(agenda, "First content\n");
        let twice = bake(&once, "Second content\n");

        assert_eq!(
            twice.matches("brain:optional-content").count(),
            1,
            "{twice}"
        );
        assert!(!twice.contains("First content"), "{twice}");
        assert!(twice.contains("Second content"), "{twice}");
        assert!(twice.contains("1. **T1** Ship"), "{twice}");
    }

    #[test]
    fn stripping_an_agenda_without_an_appendix_leaves_it_alone() {
        assert_eq!(strip_existing("# Monday\n\nBody\n"), "# Monday\n\nBody");
    }
}
