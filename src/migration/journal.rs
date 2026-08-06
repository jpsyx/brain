//! Durable machine-local rollout progress.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::Step;
use crate::workspace::WorkspaceId;

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const MIGRATION_ID: &str = "multi-workspace-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalWriteStep {
    BeforePublish,
}

/// Exact identity needed to create or resume one rollout journal.
#[derive(Debug, Clone, Copy)]
pub struct JournalRequest<'a> {
    pub path: &'a Path,
    pub workspace_id: WorkspaceId,
    pub workspace_root: &'a Path,
    pub backup_dir: &'a Path,
    pub started_at: &'a str,
    pub plan: &'a [Step],
}

/// One validated active rollout journal.
#[derive(Debug)]
pub struct MigrationJournal {
    path: PathBuf,
    document: Document,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    migration_id: String,
    workspace_id: WorkspaceId,
    workspace_root: PathBuf,
    backup_dir: PathBuf,
    started_at: String,
    plan: Vec<Step>,
    completed: Vec<Step>,
}

impl MigrationJournal {
    /// Create a new active journal or resume the exact existing rollout.
    pub fn open_or_create(request: JournalRequest<'_>) -> Result<Self> {
        let document = match fs::read(request.path) {
            Ok(bytes) => {
                let document: Document = serde_json::from_slice(&bytes).with_context(|| {
                    format!("parsing migration journal {}", request.path.display())
                })?;
                validate(&document, request)?;
                document
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let document = Document {
                    schema_version: JOURNAL_SCHEMA_VERSION,
                    migration_id: MIGRATION_ID.to_owned(),
                    workspace_id: request.workspace_id,
                    workspace_root: request.workspace_root.to_path_buf(),
                    backup_dir: request.backup_dir.to_path_buf(),
                    started_at: request.started_at.to_owned(),
                    plan: request.plan.to_vec(),
                    completed: Vec::new(),
                };
                write_document(request.path, &document)?;
                document
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("reading migration journal {}", request.path.display())
                });
            }
        };
        Ok(Self {
            path: request.path.to_path_buf(),
            document,
        })
    }

    /// Resume an existing journal while retaining its original timestamp and backup.
    pub fn resume(
        path: &Path,
        workspace_id: WorkspaceId,
        workspace_root: &Path,
        plan: &[Step],
    ) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("reading migration journal {}", path.display()))?;
        let document: Document = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing migration journal {}", path.display()))?;
        validate_identity(&document, workspace_id, workspace_root, plan)?;
        Ok(Self {
            path: path.to_path_buf(),
            document,
        })
    }

    /// Persist one verified step, requiring the exact next plan entry.
    pub fn record_completed(&mut self, step: Step) -> Result<()> {
        self.record_completed_inner(step, |_| Ok(()))
    }

    fn record_completed_inner(
        &mut self,
        step: Step,
        hook: impl FnOnce(JournalWriteStep) -> std::io::Result<()>,
    ) -> Result<()> {
        let next = self
            .document
            .plan
            .get(self.document.completed.len())
            .copied()
            .ok_or_else(|| anyhow!("migration journal has no remaining step to record"))?;
        if next != step {
            bail!("migration step {step:?} cannot complete before {next:?}");
        }
        let mut candidate = self.document.clone();
        candidate.completed.push(step);
        write_document_with_hook(&self.path, &candidate, hook)?;
        self.document = candidate;
        Ok(())
    }

    #[cfg(test)]
    fn record_completed_with_hook(
        &mut self,
        step: Step,
        hook: impl FnOnce(JournalWriteStep) -> std::io::Result<()>,
    ) -> Result<()> {
        self.record_completed_inner(step, hook)
    }

    /// Retained backup directory chosen when the rollout started.
    #[must_use]
    pub fn backup_dir(&self) -> &Path {
        &self.document.backup_dir
    }

    /// Plan suffix after the last durably verified step.
    #[must_use]
    pub fn remaining_steps(&self) -> &[Step] {
        &self.document.plan[self.document.completed.len()..]
    }

    /// Whether a step was durably recorded in the completed plan prefix.
    #[must_use]
    pub fn completed(&self, step: Step) -> bool {
        self.document.completed.contains(&step)
    }

    /// Remove the active journal only after every verified step remains done.
    pub fn mark_complete(&mut self) -> Result<()> {
        if self.remaining_steps() != [Step::MarkComplete] {
            bail!("migration journal cannot complete before final verification");
        }
        fs::remove_file(&self.path).with_context(|| {
            format!(
                "removing completed migration journal {}",
                self.path.display()
            )
        })?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow!("migration journal has no parent: {}", self.path.display()))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| {
                format!(
                    "syncing completed migration journal directory {}",
                    parent.display()
                )
            })
    }
}

fn validate(document: &Document, request: JournalRequest<'_>) -> Result<()> {
    validate_identity(
        document,
        request.workspace_id,
        request.workspace_root,
        request.plan,
    )?;
    if document.backup_dir != request.backup_dir {
        bail!("migration journal belongs to a different rollout or workspace");
    }
    Ok(())
}

fn validate_identity(
    document: &Document,
    workspace_id: WorkspaceId,
    workspace_root: &Path,
    plan: &[Step],
) -> Result<()> {
    if document.schema_version != JOURNAL_SCHEMA_VERSION {
        bail!(
            "migration journal schema {} is unsupported",
            document.schema_version
        );
    }
    if document.migration_id != MIGRATION_ID
        || document.workspace_id != workspace_id
        || document.workspace_root != workspace_root
    {
        bail!("migration journal belongs to a different rollout or workspace");
    }
    if document.plan != plan {
        bail!("migration journal plan does not match the current rollout plan");
    }
    if !document.plan.starts_with(&document.completed) {
        bail!("migration journal completed steps are not a valid plan prefix");
    }
    Ok(())
}

fn write_document(path: &Path, document: &Document) -> Result<()> {
    write_document_with_hook(path, document, |_| Ok(()))
}

fn write_document_with_hook(
    path: &Path,
    document: &Document,
    hook: impl FnOnce(JournalWriteStep) -> std::io::Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("migration journal has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating migration journal directory {}", parent.display()))?;
    let mut bytes = serde_json::to_vec_pretty(document)?;
    bytes.push(b'\n');
    let temporary = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("migration.json"),
        WorkspaceId::new()
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).with_context(|| {
            format!(
                "creating migration journal temporary {}",
                temporary.display()
            )
        })?;
        file.write_all(&bytes).with_context(|| {
            format!(
                "writing migration journal temporary {}",
                temporary.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "syncing migration journal temporary {}",
                temporary.display()
            )
        })?;
        let verified: Document =
            serde_json::from_slice(&fs::read(&temporary).with_context(|| {
                format!(
                    "verifying migration journal temporary {}",
                    temporary.display()
                )
            })?)?;
        if &verified != document {
            bail!("migration journal temporary verification failed");
        }
        hook(JournalWriteStep::BeforePublish).context("publishing verified migration journal")?;
        fs::rename(&temporary, path)
            .with_context(|| format!("publishing migration journal {}", path.display()))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("syncing migration journal directory {}", parent.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_step_record_never_replaces_the_last_verified_journal() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let path = temporary.path().join("migrations/multi-workspace-v1.json");
        let backup = temporary.path().join("migration-backups/rollout");
        fs::create_dir(&root).unwrap();
        let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
        let plan = [Step::BackupPortableData, Step::Verify];
        let mut journal = MigrationJournal::open_or_create(JournalRequest {
            path: &path,
            workspace_id,
            workspace_root: &root,
            backup_dir: &backup,
            started_at: "2026-08-06T12:00:00Z",
            plan: &plan,
        })
        .unwrap();
        let before = fs::read(&path).unwrap();

        let error = journal
            .record_completed_with_hook(Step::BackupPortableData, |step| {
                if step == JournalWriteStep::BeforePublish {
                    return Err(std::io::Error::other("injected journal publish failure"));
                }
                Ok(())
            })
            .unwrap_err();

        assert!(format!("{error:#}").contains("injected journal publish failure"));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(journal.remaining_steps(), plan);
    }
}
