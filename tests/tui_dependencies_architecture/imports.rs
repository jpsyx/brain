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
fn dependency_guard_recognizes_grouped_tui_root_globs() {
    let ordinary = Path::new("src/tui/handlers/input.rs");
    let fixtures = [
        "use crate::tui::*;",
        "use crate::tui::{App, *};",
        "use\ncrate :: tui :: { App, * };",
        "use crate::{tui::*};",
    ];
    assert_fixture_violations(ordinary, &fixtures, DependencyViolation::TuiRootGlobImport);
}

#[test]
fn dependency_guard_recognizes_grouped_parent_globs() {
    let ordinary = Path::new("src/tui/handlers/input.rs");
    let fixtures = [
        "use super::*;",
        "use super::{App, *};",
        "use super::super::{Fixture, *};",
        "use {super::*};",
    ];
    assert_fixture_violations(ordinary, &fixtures, DependencyViolation::ParentGlobImport);
}

#[test]
fn dependency_guard_recognizes_arbitrary_public_visibility() {
    let root_module = Path::new("src/tui/mod.rs");
    let fixtures = [
        "pub(crate) use action::*;",
        "pub(in crate) use action::*;",
        "pub use state::{AppContext, *};",
        "pub(super) use\n draw :: { draw, * };",
        "#[cfg(test)] mod tests;\npub(crate) use action::*;\npub struct App {}",
    ];
    assert_fixture_violations(
        root_module,
        &fixtures,
        DependencyViolation::RootWildcardReexport,
    );
}

#[test]
fn dependency_guard_ignores_explicit_and_test_only_imports() {
    let ordinary = Path::new("src/tui/handlers/input.rs");
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

fn assert_fixture_violations(path: &Path, fixtures: &[&str], expected: DependencyViolation) {
    let missed = fixtures
        .iter()
        .copied()
        .filter(|source| dependency_violations(path, source) != vec![expected])
        .collect::<Vec<_>>();
    assert!(
        missed.is_empty(),
        "dependency guard missed fixtures for {expected:?}:\n{}",
        missed.join("\n")
    );
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
        let wildcard_paths = wildcard_paths(statement);
        if wildcard_paths.is_empty() {
            continue;
        }

        if wildcard_paths.iter().any(|path| {
            path.first().is_some_and(|segment| segment == "crate")
                && path.get(1).is_some_and(|segment| segment == "tui")
        }) {
            violations.push(DependencyViolation::TuiRootGlobImport);
        } else if wildcard_paths
            .iter()
            .any(|path| path.first().is_some_and(|segment| segment == "super"))
        {
            violations.push(DependencyViolation::ParentGlobImport);
        } else if path == Path::new("src/tui/mod.rs") && is_public_use(&tokens, index) {
            violations.push(DependencyViolation::RootWildcardReexport);
        }
    }

    violations
}

fn wildcard_paths(tokens: &[Token]) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    collect_wildcard_paths(tokens, &[], &mut paths);
    paths
}

fn collect_wildcard_paths(tokens: &[Token], prefix: &[String], paths: &mut Vec<Vec<String>>) {
    let mut depth = 0_usize;
    let mut start = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                collect_wildcard_branch(&tokens[start..index], prefix, paths);
                start = index + 1;
            }
            ";" if depth == 0 => {
                collect_wildcard_branch(&tokens[start..index], prefix, paths);
                return;
            }
            _ => {}
        }
    }
    collect_wildcard_branch(&tokens[start..], prefix, paths);
}

fn collect_wildcard_branch(tokens: &[Token], prefix: &[String], paths: &mut Vec<Vec<String>>) {
    let mut path = prefix.to_vec();
    let mut index = 0_usize;
    while let Some(token) = tokens.get(index) {
        match token.text.as_str() {
            "*" => {
                paths.push(path);
                return;
            }
            "{" => {
                let Some(close) = matching_token(tokens, index, "{", "}") else {
                    return;
                };
                collect_wildcard_paths(&tokens[index + 1..close], &path, paths);
                return;
            }
            "::" => {}
            "as" | ";" => return,
            segment => path.push(segment.to_owned()),
        }
        index += 1;
    }
}

fn is_public_use(tokens: &[Token], use_index: usize) -> bool {
    let Some(previous) = use_index.checked_sub(1) else {
        return false;
    };
    if tokens[previous].text == "pub" {
        return true;
    }
    if tokens[previous].text != ")" {
        return false;
    }

    let mut depth = 0_usize;
    for index in (0..=previous).rev() {
        match tokens[index].text.as_str() {
            ")" => depth += 1,
            "(" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index
                        .checked_sub(1)
                        .is_some_and(|before| tokens[before].text == "pub");
                }
            }
            _ => {}
        }
    }
    false
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
