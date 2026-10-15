//! Mandatory per-workspace compatibility and local-only downgrade settlement.
use crate::{
    skills::{
        install_transaction::{InstallIdentity, LockedInstall},
        receipt::WorkspaceStamp,
        reconcile::RefreshMode,
        repair::RefreshAttempt,
    },
    workspace::{RegistryStore, WorkspaceContext},
};
use anyhow::Result;
use std::path::Path;

pub(super) fn up(home: &Path) -> Result<()> {
    up_with_attempt(home, None)
}
pub(super) fn up_with_attempt(home: &Path, attempt: Option<&mut RefreshAttempt>) -> Result<()> {
    up_with_store(home, &RegistryStore::real(), attempt)
}
fn up_with_store(
    home: &Path,
    store: &RegistryStore,
    attempt: Option<&mut RefreshAttempt>,
) -> Result<()> {
    let mut ordinary = RefreshAttempt::new(RefreshMode::Compatibility);
    let report = attempt.unwrap_or(&mut ordinary).run(Some(home), store);
    if report.failed() {
        return Err(crate::skills::diagnostic::RefreshFailure(report.clone()).into());
    }
    Ok(())
}
pub(super) fn down(home: &Path) -> Result<()> {
    down_with_store(home, &RegistryStore::real())
}
fn down_with_store(home: &Path, store: &RegistryStore) -> Result<()> {
    if !store.path().exists() {
        return Ok(());
    }
    let registry = RegistryStore::load_readable(store.path())?;
    for (name, record) in &registry.workspaces {
        match std::fs::metadata(&record.root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            other => {
                other?;
            }
        }
        let workspace = WorkspaceContext::new(
            home,
            record.workspace_id,
            name.clone(),
            &record.root,
            &record.local_user_id,
            home,
        )?;
        let identity = InstallIdentity::registered(
            &record.root,
            record.workspace_id,
            workspace.paths().cache_dir(),
        )?;
        identity.prepare_cache()?;
        let _operations = crate::tasks::removal::prepare_downgrade(&workspace)?;
        let mut stamp = WorkspaceStamp::selected(&workspace);
        let mut locked = LockedInstall::acquire(&identity)?;
        locked.recover(&mut stamp)?;
        locked.inspect(&crate::skills::install_transaction::InstallVersion::current())?;
        // Ownership hashes remain authoritative for subsequent target publication.
        stamp.invalidate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{WorkspaceId, WorkspaceManifest, WorkspacePaths};
    fn fixture() -> (
        tempfile::TempDir,
        RegistryStore,
        std::path::PathBuf,
        WorkspacePaths,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let id = WorkspaceId::new();
        WorkspaceManifest::new(id).write_new(&root).unwrap();
        let store = RegistryStore::from_path(temp.path().join("env.json"));
        std::fs::write(
            store.path(),
            serde_json::to_vec(&serde_json::json!({
                "schema_version":4, "default_workspace":"alpha", "workspaces": {
                    "alpha": {"workspace_id":id, "root":root, "local_user_id":"member"}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let paths = WorkspacePaths::new(temp.path(), id);
        (temp, store, root, paths)
    }
    #[test]
    fn up_reconciles_idempotently_and_repairs_missing_generated_content() {
        let (home, store, root, _) = fixture();
        up_with_store(home.path(), &store, None).unwrap();
        let todo = root.join(".agents/skills/todo/SKILL.md");
        let before = std::fs::metadata(&todo).unwrap().modified().unwrap();
        up_with_store(home.path(), &store, None).unwrap();
        assert_eq!(
            std::fs::metadata(&todo).unwrap().modified().unwrap(),
            before
        );
        std::fs::remove_file(&todo).unwrap();
        up_with_store(home.path(), &store, None).unwrap();
        assert!(todo.exists());
    }
    #[test]
    fn down_idempotently_invalidates_receipts_without_discarding_ownership() {
        let (home, store, root, paths) = fixture();
        up_with_store(home.path(), &store, None).unwrap();
        let manifest = paths.cache_dir().join("skills-install/managed.json");
        let ownership = std::fs::read(&manifest).unwrap();
        let todo = std::fs::read(root.join(".agents/skills/todo/SKILL.md")).unwrap();
        for _ in 0..2 {
            down_with_store(home.path(), &store).unwrap();
        }
        assert_eq!(std::fs::read(manifest).unwrap(), ownership);
        assert_eq!(
            std::fs::read(root.join(".agents/skills/todo/SKILL.md")).unwrap(),
            todo
        );
        let db = rusqlite::Connection::open(paths.state_db()).unwrap();
        let count: i64 = db.query_row("SELECT count(*) FROM meta WHERE key IN ('skills_install_receipt', 'skills_synced_version')", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 0);
    }
}
