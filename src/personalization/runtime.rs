//! Explicit workspace-selected tag styles for task rendering.

use super::tags::TagStyles;

/// Load resolved tag styles from one workspace's portable personalization.
#[must_use]
pub fn load_tag_styles(workspace: &crate::workspace::WorkspaceContext) -> TagStyles {
    TagStyles::with_overrides(&super::store::load(workspace).tag_styles)
}
