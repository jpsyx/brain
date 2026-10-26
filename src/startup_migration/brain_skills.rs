//! The split-package boundary shares the historical callback's invocation attempt.
use anyhow::Result;
use std::path::Path;

pub(crate) fn predates_split(version: &str) -> Result<bool> {
    Ok(super::version::Version::parse(version)? < super::BRAIN_SKILLS_VERSION)
}

pub(super) fn up(home: &Path) -> Result<()> {
    super::task_deletion_skills::up(home)
}

pub(super) fn down(home: &Path) -> Result<()> {
    // Exact target publication belongs to the installer refresh protocol.
    super::task_deletion_skills::down(home)
}
