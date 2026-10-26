//! Pure, source-preserving compatibility projection for split Brain skills.

mod hooks;
mod names;
mod projection;
mod provenance;
pub(crate) mod snapshot;

use crate::skills::extension::Extension;
pub(crate) use provenance::{ProjectionProvenance, SourcePatch};
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionDirection {
    Current,
    Legacy105,
    #[cfg_attr(not(test), allow(dead_code))] // The audited target adapter is integrated later.
    Legacy101,
}

#[derive(Debug, Default)]
pub(crate) struct ExtensionProjection {
    pub by_skill: BTreeMap<String, Extension>,
    pub workspace: Extension,
    pub legacy_task_context: String,
    pub legacy_source_patches: Vec<SourcePatch>,
    pub sources: BTreeMap<String, Vec<u8>>,
    pub origins: Vec<ExtensionOrigin>,
}

/// Origin of one logical block. Resource bases remain tied to its source path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionOrigin {
    pub source_path: String,
    pub source_hook: Option<String>,
    pub content_range: Range<usize>,
    pub logical_hook: String,
    pub owner: String,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod names_tests;

pub(crate) fn project_extensions(
    sources: &BTreeMap<String, String>,
    direction: ProjectionDirection,
) -> anyhow::Result<ExtensionProjection> {
    let mut projection = projection::current(sources)?;
    if direction != ProjectionDirection::Current {
        projection::project_down(sources, &mut projection)?;
    }
    Ok(projection)
}

pub(crate) fn expand_legacy_names(names: &[String]) -> Vec<String> {
    names::expand(names)
}

#[cfg(test)]
mod down_tests;

pub(crate) fn project_allowed_skills(
    config: &serde_json::Value,
) -> anyhow::Result<Option<crate::skills::install_transaction::package::JsonFieldPatch>> {
    names::project_allowed_skills(config)
}

#[cfg_attr(not(test), allow(dead_code))] // The audited downgrade coordinator supplies recorded history.
pub(crate) fn reverse_allowed_skills(
    config: &serde_json::Value,
    history: &[crate::skills::install_transaction::package::JsonFieldPatch],
) -> anyhow::Result<Option<crate::skills::install_transaction::package::JsonFieldPatch>> {
    names::reverse_allowed_skills(config, history)
}
