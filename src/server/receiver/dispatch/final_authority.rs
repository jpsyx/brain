//! Exact persisted-intent and live-lease check at socket admission boundaries.

pub(super) fn final_admission(
    control: &std::sync::Mutex<crate::server::control::ControlServer>,
    workspace: &crate::server::workspace_route::ResolvedWorkspaceRoute,
    now: std::time::Instant,
) -> std::io::Result<()> {
    workspace
        .revalidate_receiver_intent()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .revalidate_workspace_route(workspace, now)
        .map_err(|error| std::io::Error::other(error.to_string()))
}
