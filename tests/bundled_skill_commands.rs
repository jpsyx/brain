//! Every `brain …` command a bundled skill names must actually exist.
//!
//! The skills are instructions an agent follows literally. A command that was
//! renamed, or never existed, does not fail loudly — the agent improvises, and
//! improvising around a task mutation is how data gets edited by hand.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Words that follow `brain` but are flags or placeholders, not subcommands.
fn is_subcommand_word(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && !word.starts_with('<')
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Pull `brain <a> [b]` out of every inline-code span and fenced code block.
///
/// Prose is deliberately excluded — "the brain is a directory" is not a
/// command — so only text the reader is meant to type is checked.
fn referenced_commands(markdown: &str) -> BTreeSet<Vec<String>> {
    let mut found = BTreeSet::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let indented = line.starts_with("    ") || line.starts_with('\t');
        if fenced || indented {
            collect_from(line, &mut found);
            continue;
        }
        for span in code_spans(line) {
            collect_from(span, &mut found);
        }
    }
    found
}

/// The contents of every `` `…` `` span on one line.
fn code_spans(line: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('`') else {
            break;
        };
        spans.push(&rest[..close]);
        rest = &rest[close + 1..];
    }
    spans
}

/// Every `brain <a> [b]` in one chunk of code text.
fn collect_from(text: &str, found: &mut BTreeSet<Vec<String>>) {
    let mut words = text.split_whitespace();
    while let Some(word) = words.next() {
        if word != "brain" {
            continue;
        }
        let command: Vec<String> = words
            .clone()
            .take(2)
            .take_while(|word| is_subcommand_word(word))
            .map(str::to_owned)
            .collect();
        if !command.is_empty() {
            found.insert(command);
        }
    }
}

fn markdown_files(directory: &Path, into: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            markdown_files(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            into.push(path);
        }
    }
}

#[test]
fn every_command_the_bundled_skills_name_resolves() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    markdown_files(&manifest.join("skills"), &mut files);
    assert!(files.len() > 5, "expected the bundled skill tree");

    let mut referenced = BTreeSet::new();
    for file in &files {
        let markdown = std::fs::read_to_string(file).expect("read skill");
        referenced.extend(referenced_commands(&markdown));
    }
    assert!(
        referenced.len() > 20,
        "expected the skills to name many commands, found {}",
        referenced.len()
    );

    let mut missing = Vec::new();
    for words in &referenced {
        // `--help` is side-effect free and never touches a workspace, so this
        // asks clap the question directly.
        let status = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(words)
            .arg("--help")
            .env("NO_COLOR", "1")
            .output()
            .expect("run brain --help");
        if !status.status.success() {
            missing.push(format!("brain {}", words.join(" ")));
        }
    }

    assert!(
        missing.is_empty(),
        "bundled skills name commands that do not exist:\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_extractor_finds_commands_and_ignores_prose() {
    let found = referenced_commands(
        "Run `brain tasks complete T1` then `brain backlog purge`.\n\
         The brain is a directory. Use `brain -w family habits skip H1`.",
    );

    assert!(found.contains(&vec!["tasks".to_owned(), "complete".to_owned()]));
    assert!(found.contains(&vec!["backlog".to_owned(), "purge".to_owned()]));
    // A flag stops the capture rather than being treated as a subcommand.
    assert!(!found.iter().any(|words| words[0].starts_with('-')));
    // "brain is a directory" contributes nothing.
    assert!(!found.contains(&vec!["is".to_owned(), "a".to_owned()]));
}
