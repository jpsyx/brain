use std::path::Path;

use super::source::production_tui_sources;
use super::tokens::{Token, matching_token, rust_tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DependencyViolation {
    TuiRootGlobImport,
    ParentGlobImport,
    RootWildcardReexport,
}

#[test]
fn production_tui_dependencies_are_explicit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = root.join("src/tui");
    let mut violations = Vec::new();

    for path in production_tui_sources(&tui_root) {
        let source = std::fs::read_to_string(&path).expect("read production TUI source");
        let relative = path
            .strip_prefix(root)
            .expect("TUI source below repository");
        for violation in dependency_violations(relative, &source) {
            violations.push(format!("{}: {violation:?}", relative.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "production TUI dependency globs must be replaced with explicit owner paths:\n{}",
        violations.join("\n")
    );
}

#[test]
fn dependency_guard_recognizes_realistic_rust_forms() {
    let ordinary = Path::new("src/tui/handlers/input.rs");
    let root_module = Path::new("src/tui/mod.rs");

    for source in [
        "use crate::tui::*;",
        "use crate::tui::{App, *};",
        "use\ncrate :: tui :: { App, * };",
    ] {
        assert_eq!(
            dependency_violations(ordinary, source),
            vec![DependencyViolation::TuiRootGlobImport],
            "missed TUI-root glob fixture: {source}"
        );
    }

    for source in [
        "use super::*;",
        "use super::{App, *};",
        "use super::super::{Fixture, *};",
    ] {
        assert_eq!(
            dependency_violations(ordinary, source),
            vec![DependencyViolation::ParentGlobImport],
            "missed parent glob fixture: {source}"
        );
    }

    for source in [
        "pub(crate) use action::*;",
        "pub use state::{AppContext, *};",
        "pub(super) use\n draw :: { draw, * };",
        "#[cfg(test)] mod tests;\npub(crate) use action::*;\npub struct App {}",
    ] {
        assert_eq!(
            dependency_violations(root_module, source),
            vec![DependencyViolation::RootWildcardReexport],
            "missed root wildcard re-export fixture: {source}"
        );
    }

    let allowed = r#"
        use crate::tui::state::{AppContext, TasksState};
        use super::receiver::{ReceiverEffect, ReceiverRuntime};
        const EXAMPLE: &str = "use crate::tui::*;";
        // use super::*
        #[cfg(test)]
        mod tests {
            use super::*;
        }
    "#;
    assert!(dependency_violations(ordinary, allowed).is_empty());
}

fn dependency_violations(path: &Path, source: &str) -> Vec<DependencyViolation> {
    let tokens = rust_tokens(source);
    let test_ranges = cfg_test_ranges(&tokens);
    let mut violations = Vec::new();

    for (index, token) in tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.text == "use")
    {
        if test_ranges
            .iter()
            .any(|(start, end)| (*start..*end).contains(&token.start))
        {
            continue;
        }
        let end = tokens[index..]
            .iter()
            .position(|candidate| candidate.text == ";")
            .map_or(tokens.len(), |relative| index + relative + 1);
        let statement = &tokens[index + 1..end];
        if !statement.iter().any(|candidate| candidate.text == "*") {
            continue;
        }

        if starts_with_path(statement, &["crate", "::", "tui", "::"])
            || starts_with_path(statement, &["crate", "::", "tui", "{"])
        {
            violations.push(DependencyViolation::TuiRootGlobImport);
        } else if statement
            .first()
            .is_some_and(|candidate| candidate.text == "super")
        {
            violations.push(DependencyViolation::ParentGlobImport);
        } else if path == Path::new("src/tui/mod.rs") && is_public_use(&tokens, index) {
            violations.push(DependencyViolation::RootWildcardReexport);
        }
    }

    violations
}

fn starts_with_path(tokens: &[Token], expected: &[&str]) -> bool {
    tokens.len() >= expected.len()
        && tokens
            .iter()
            .map(|token| token.text.as_str())
            .zip(expected.iter().copied())
            .all(|(actual, expected)| actual == expected)
}

fn is_public_use(tokens: &[Token], use_index: usize) -> bool {
    use_index
        .checked_sub(1)
        .is_some_and(|index| tokens[index].text == "pub")
        || use_index
            .checked_sub(4)
            .is_some_and(|index| tokens[index].text == "pub")
}

fn cfg_test_ranges(tokens: &[Token]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let pattern = ["#", "[", "cfg", "(", "test", ")", "]"];
    for index in 0..tokens.len().saturating_sub(pattern.len() - 1) {
        if !tokens[index..]
            .iter()
            .map(|token| token.text.as_str())
            .zip(pattern)
            .all(|(actual, expected)| actual == expected)
        {
            continue;
        }
        let item_start = index + pattern.len();
        if tokens
            .get(item_start)
            .is_some_and(|token| token.text == "mod")
        {
            let terminator = tokens[item_start..]
                .iter()
                .position(|token| matches!(token.text.as_str(), ";" | "{"))
                .map(|relative| item_start + relative);
            match terminator {
                Some(end) if tokens[end].text == ";" => {
                    ranges.push((tokens[index].start, tokens[end].end));
                }
                Some(open) => {
                    if let Some(close) = matching_token(tokens, open, "{", "}") {
                        ranges.push((tokens[index].start, tokens[close].end));
                    }
                }
                None => {}
            }
        } else if let Some(end) = tokens[item_start..]
            .iter()
            .position(|token| token.text == ";")
            .map(|relative| item_start + relative)
        {
            ranges.push((tokens[index].start, tokens[end].end));
        }
    }
    ranges
}
