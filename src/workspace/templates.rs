//! The document a new workspace is created with.
//!
//! `AGENTS.md` is how an agent learns what this directory is and how to behave
//! in it, and it describes the layout well enough to orient a person too. Every
//! supported frontend resolves it, which is why the instructions live there
//! rather than in a frontend-specific filename — and why it is the only
//! orientation document Brain seeds. It is written only when absent: from the
//! moment it exists it is the user's document, not Brain's, so an edited copy is
//! never replaced.

use std::path::Path;

use anyhow::{Context, Result};

/// Instructions an agent follows inside a workspace.
pub(crate) const AGENTS: &str = include_str!("../../templates/workspace/AGENTS.md");

/// Write the document into a workspace root, leaving any existing copy alone.
pub(crate) fn seed_documents(root: &Path) -> Result<()> {
    let path = root.join("AGENTS.md");
    if path.exists() {
        return Ok(());
    }
    std::fs::write(&path, AGENTS)
        .with_context(|| format!("seeding AGENTS.md at {}", path.display()))
}

#[cfg(test)]
mod tests;
