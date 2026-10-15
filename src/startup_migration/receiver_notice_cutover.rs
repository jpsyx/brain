use std::path::Path;

use anyhow::{Context as _, Result};

pub(super) fn up(home: &Path) -> Result<()> {
    for state in super::receiver_model::workspace_states(home) {
        crate::state::Db::open_path_with_legacy_identity(
            &state.path,
            &state.workspace_id,
            &state.local_user_id,
        )
        .context("upgrade receiver notice state")?;
    }
    Ok(())
}

pub(super) fn down(home: &Path) -> Result<()> {
    for state in super::receiver_model::workspace_states(home) {
        down_state(&state.path)?;
    }
    Ok(())
}

fn down_state(path: &Path) -> Result<()> {
    crate::state::receiver_notice_cutover_schema_down(path)
        .context("downgrade receiver notice state")
}

#[cfg(test)]
mod tests {
    #[test]
    fn exact_notice_downgrade_preserves_context_without_private_state_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private-state.db");
        drop(crate::state::Db::open_path(&path).unwrap());
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("DROP TABLE receiver_jobs; CREATE TABLE receiver_jobs (damaged TEXT); PRAGMA user_version = 13;")
            .unwrap();
        let error = super::down_state(&path).unwrap_err();
        let rendered = format!("Error: {error:#}");
        assert!(rendered.contains("downgrade receiver notice state"));
        assert!(
            !rendered.contains(path.to_string_lossy().as_ref()),
            "{rendered}"
        );
    }
}
