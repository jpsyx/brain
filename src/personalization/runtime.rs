//! Explicit workspace-selected tag styles for task rendering.

use super::tags::TagStyles;

/// Load resolved tag styles from the local person's persona.
///
/// Tag styling is how *this* machine's user wants their board to read, so it
/// comes from their persona rather than being merged across the workspace.
#[must_use]
pub fn load_tag_styles(workspace: &crate::workspace::WorkspaceContext) -> TagStyles {
    TagStyles::with_overrides(&super::store::local_persona(workspace).tag_styles)
}
