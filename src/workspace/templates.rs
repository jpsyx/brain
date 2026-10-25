//! Seeds a missing workspace document with the pure managed instruction plan.
//! Existing documents remain untouched by this initialization seam; coordinated
//! package reconciliation owns later updates and conflict handling.

use std::path::Path;

use anyhow::{Context, Result};

use super::instructions::{plan_document, render_contract};

/// Write the document into a workspace root, leaving any existing copy alone.
pub(crate) fn seed_documents(root: &Path) -> Result<()> {
    let path = root.join("AGENTS.md");
    if path.exists() {
        return Ok(());
    }
    let plan = plan_document(None, &render_contract(), None)?;
    std::fs::write(&path, plan.after)
        .with_context(|| format!("seeding AGENTS.md at {}", path.display()))
}

#[cfg(test)]
mod tests;
