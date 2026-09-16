//! Widen every stored `agent_kind` contract to include pi, and narrow it again
//! on a downgrade.
//!
//! Opening a workspace state database reconciles its table contracts against
//! the current frontend registry, so the upgrade is just an open. A downgrade
//! runs from this binary (the installer invokes the existing binary before
//! replacing it), so it restores the contract the previous version creates and
//! drops the rows only pi could own.

use std::path::Path;

use anyhow::{Context as _, Result};

pub(super) fn up(home: &Path) -> Result<()> {
    for state in super::receiver_model::workspace_states(home) {
        crate::state::Db::open_path_with_legacy_identity(
            &state.path,
            &state.workspace_id,
            &state.local_user_id,
        )
        .with_context(|| format!("upgrade frontend state {}", state.path.display()))?;
    }
    Ok(())
}

pub(super) fn down(home: &Path) -> Result<()> {
    for state in super::receiver_model::workspace_states(home) {
        crate::state::frontend_contract_down(&state.path)
            .with_context(|| format!("downgrade frontend state {}", state.path.display()))?;
    }
    Ok(())
}
