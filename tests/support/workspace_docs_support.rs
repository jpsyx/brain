use std::path::Path;
use std::process::{Command, Output};

const CURRENT_DOCS: &[&str] = &[
    "README.md",
    "docs/glossary.md",
    "docs/architecture.md",
    "docs/features.md",
    "docs/data-model.md",
    "docs/config.md",
    "docs/decisions.md",
    "docs/integrations.md",
    "docs/keybindings.md",
    "docs/README.md",
    "docs/testing.md",
];

fn brain_help(args: &[&str]) -> String {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(args)
        .output()
        .expect("run brain help");
    assert!(
        status.success(),
        "brain help failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    String::from_utf8(stdout).expect("help is UTF-8")
}

fn read_doc(relative: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn read_doc_normalized(relative: &str) -> String {
    read_doc(relative)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn current_docs() -> String {
    CURRENT_DOCS
        .iter()
        .map(|path| read_doc(path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn current_docs_normalized() -> String {
    CURRENT_DOCS
        .iter()
        .map(|path| read_doc_normalized(path))
        .collect::<Vec<_>>()
        .join("\n")
}
