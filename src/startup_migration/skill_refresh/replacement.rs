//! Final executable handoff inside the existing refresh coordinator's ownership.
use super::target::{Target, digest};
use crate::cli::{RefreshPhase, RefreshWorkspaceSkillsArgs};
use anyhow::{Result, ensure};
use std::{fs, path::PathBuf};

#[cfg(test)]
mod tests;

pub(super) struct Replacement {
    path: PathBuf,
    before: Option<String>,
}

impl Replacement {
    pub(super) fn prepare(
        args: &RefreshWorkspaceSkillsArgs,
        target: &Target,
    ) -> Result<Option<Self>> {
        let Some(path) = &args.replace_binary else {
            return Ok(None);
        };
        ensure!(
            args.phase == RefreshPhase::Apply,
            "replacement requires apply phase"
        );
        ensure!(
            path.is_absolute() && path != &target.path,
            "invalid installed executable path"
        );
        let replacement = Self {
            path: path.clone(),
            before: args.expected_installed_sha256.clone(),
        };
        replacement.verify_before()?;
        if replacement.before.is_some() {
            super::super::invocation::verify_executable(
                path,
                args.from_version
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("replacement requires installed version"))?,
            )?;
        }
        Ok(Some(replacement))
    }

    fn verify_before(&self) -> Result<()> {
        if let Some(expected) = &self.before {
            ensure!(
                digest(&self.path)? == *expected,
                "installed executable changed; retry installation"
            );
        } else {
            ensure!(
                matches!(fs::symlink_metadata(&self.path), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                "installed executable appeared; retry installation"
            );
        }
        Ok(())
    }

    pub(super) fn publish(
        self,
        target: &Target,
        _ownership: super::super::invocation::ExclusiveGuard,
    ) -> Result<()> {
        #[cfg(test)]
        tests::before_replace();
        target.verify()?;
        self.verify_before()?;
        let expected = digest(&target.path)?;
        fs::rename(&target.path, &self.path)?;
        fs::File::open(self.path.parent().expect("absolute executable parent"))?.sync_all()?;
        ensure!(
            digest(&self.path)? == expected,
            "installed executable digest changed"
        );
        Ok(())
    }
}
