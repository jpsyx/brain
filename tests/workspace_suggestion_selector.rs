//! Every command Brain suggests for a workspace-scoped family must carry the
//! workspace selector.
//!
//! Telling someone working in `family` to run `brain sync setup` sends them to
//! the default workspace, where the command either fails or, worse, succeeds
//! against the wrong root. `crate::workspace::suggest` exists to append the
//! selector; this guard proves nobody hand-rolled a suggestion around it.

use std::path::{Path, PathBuf};

/// Command families whose every invocation resolves one workspace. `env`,
/// `skills`, `server`, and `workspace` are excluded on purpose: those are
/// machine-local or registry-level, so a selector would be noise.
const WORKSPACE_SCOPED: [&str; 8] = [
    "sync", "config", "persona", "tasks", "habits", "todo", "reindex", "user",
];

/// Literals that *name* a command instead of telling the user to run one now.
/// A rename notice reads "`brain sync init` was renamed to `brain sync repair`";
/// a selector there would be nonsense.
const NAMES_A_COMMAND: [(&str, &str); 2] = [
    ("command/sync.rs", "brain sync init"),
    ("command/sync.rs", "brain sync repair"),
];

#[test]
fn test_only_section_directories_are_excluded_without_hiding_production_modules() {
    assert!(is_test_only_directory(Path::new(
        "src/sync/check/tests_sections"
    )));
    assert!(is_test_only_directory(Path::new(
        "src/tui/tests/keymap_sections"
    )));
    assert!(!is_test_only_directory(Path::new(
        "src/sync/command/reporting_sections"
    )));
    assert!(!is_test_only_directory(Path::new(
        "src/sync/command/sections"
    )));
}

fn production_source(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut production = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        // Inline test modules assert on suggestions built with no workspace
        // selected, where the selector is correctly absent. Stop at the test
        // module itself, not at every `#[cfg(test)]` helper above it.
        if line.trim() == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim_start().starts_with("mod tests"))
        {
            break;
        }
        if !line.trim_start().starts_with("//") {
            production.push(*line);
        }
    }
    Some(production.join("\n"))
}

fn is_test_only_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if matches!(name, "tests_parts" | "tests_sections") {
        return true;
    }
    name.ends_with("_sections")
        && path
            .ancestors()
            .skip(1)
            .any(|ancestor| ancestor.file_name().is_some_and(|name| name == "tests"))
}

fn rust_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory).expect("readable source directory");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !is_test_only_directory(&path) {
                rust_sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !name.starts_with("tests")
            && name != "selector.rs"
            // `--help` examples are printed before any workspace is resolved,
            // so they illustrate the command rather than target a workspace.
            && !path.to_string_lossy().contains("/cli/")
        {
            found.push(path);
        }
    }
}

/// Suggested command lines inside backticks, e.g. ``run `brain sync setup` ``.
fn suggested_commands(source: &str) -> Vec<String> {
    let mut suggestions = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find("`brain ") {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('`') {
            suggestions.push(rest[..end].to_owned());
            rest = &rest[end + 1..];
        }
    }
    suggestions
}

#[test]
fn suggested_workspace_scoped_commands_carry_the_selector() {
    let mut sources = Vec::new();
    rust_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut sources,
    );
    assert!(sources.len() > 100, "expected the whole src tree");

    let mut offenders = Vec::new();
    for path in sources {
        let Some(source) = production_source(&path) else {
            continue;
        };
        for suggestion in suggested_commands(&source) {
            let family = suggestion
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .trim_end_matches(|c: char| !c.is_alphanumeric());
            if !WORKSPACE_SCOPED.contains(&family) {
                continue;
            }
            if suggestion.contains("-w ") || suggestion.contains("--workspace") {
                continue;
            }
            let display = path.to_string_lossy();
            if NAMES_A_COMMAND
                .iter()
                .any(|(file, named)| display.ends_with(file) && suggestion == *named)
            {
                continue;
            }
            offenders.push(format!("{display}: `{suggestion}`"));
        }
    }

    assert!(
        offenders.is_empty(),
        "these suggestions bypass workspace::suggest and would send the user to the default workspace:\n{}",
        offenders.join("\n")
    );
}
