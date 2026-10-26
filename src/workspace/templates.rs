//! Seeds a missing workspace document with the pure managed instruction plan.
//! Existing documents remain untouched by this initialization seam; coordinated
//! package reconciliation owns later updates and conflict handling.

use std::path::Path;

use anyhow::{Context, Result};

use super::instructions::{plan_document, render_contract};

/// Write the document into a workspace root, leaving any existing copy alone.
pub(crate) fn seed_documents(root: &Path) -> Result<()> {
    let path = root.join("AGENTS.md");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "AGENTS.md must be a regular file"
            );
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let plan = plan_document(None, &render_contract(), None)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::io::Write::write_all(&mut file, &plan.after)
        .with_context(|| format!("seeding AGENTS.md at {}", path.display()))
}

#[cfg(test)]
mod tests;
