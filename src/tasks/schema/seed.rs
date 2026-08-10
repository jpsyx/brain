//! Seeding the canonical task schema document into a workspace that has none.
//!
//! `tasks/SCHEMA.json` is required input for every schema decision, but nothing
//! created it: `initialize_if_empty` seeded the CSVs and left the document
//! missing, so a workspace Brain made itself could not finish `brain sync
//! setup`. Brain now carries the canonical document and writes it when absent,
//! the same write-only-when-absent rule the portable manifest follows, so a
//! copy that arrived over sync is authoritative and never replaced.

use std::path::Path;

use anyhow::{Context, Result};

/// The canonical current-schema document, generic to any workspace.
pub(crate) const CANONICAL_DOCUMENT: &str = include_str!("task_schema.json");

const DOCUMENT: &str = "tasks/SCHEMA.json";

/// Whether this workspace declares its task schema.
pub(crate) fn document_present(root: &Path) -> bool {
    root.join(DOCUMENT).is_file()
}

/// Write the canonical document when the workspace has none.
///
/// Returns whether it was written. A root with no `tasks/` directory is not a
/// task store yet, so it is left untouched rather than conjured into one.
pub(crate) fn ensure_schema_document(root: &Path) -> Result<bool> {
    if document_present(root) {
        return Ok(false);
    }
    if !root.join("tasks").is_dir() {
        return Ok(false);
    }
    let path = root.join(DOCUMENT);
    std::fs::write(&path, CANONICAL_DOCUMENT)
        .with_context(|| format!("seeding the task schema document at {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests;
