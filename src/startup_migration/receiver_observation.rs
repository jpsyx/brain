use std::path::Path;

use anyhow::{Context, Result};

pub(super) fn up(home: &Path) -> Result<()> {
    for state in super::receiver_model::workspace_states(home) {
        crate::state::Db::open_path_with_legacy_identity(
            &state.path,
            &state.workspace_id,
            &state.local_user_id,
        )
        .with_context(|| {
            format!(
                "upgrade receiver observation state {}",
                state.path.display()
            )
        })?;
    }
    Ok(())
}

pub(super) fn down(home: &Path) -> Result<()> {
    for state in super::receiver_model::workspace_states(home) {
        crate::state::receiver_observation_schema_down(&state.path).with_context(|| {
            format!(
                "downgrade receiver observation state {}",
                state.path.display()
            )
        })?;
    }
    Ok(())
}
