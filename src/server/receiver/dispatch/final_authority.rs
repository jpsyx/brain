//! Exact persisted-intent and live-lease check at socket admission boundaries.

pub(super) fn final_admission(
    control: &std::sync::Mutex<crate::server::control::ControlServer>,
    workspace: &crate::server::workspace_route::ResolvedWorkspaceRoute,
    clock: &impl Fn() -> std::time::Instant,
    #[cfg(test)] after_intent_reload: Option<&(dyn Fn() + Send + Sync)>,
) -> std::io::Result<()> {
    workspace
        .revalidate_receiver_intent()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    #[cfg(test)]
    if let Some(hook) = after_intent_reload {
        hook();
    }
    revalidate_live_authority(control, workspace, clock())
}

pub(super) fn revalidate_live_authority(
    control: &std::sync::Mutex<crate::server::control::ControlServer>,
    workspace: &crate::server::workspace_route::ResolvedWorkspaceRoute,
    now: std::time::Instant,
) -> std::io::Result<()> {
    control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .revalidate_workspace_route(workspace, now)
        .map_err(|error| std::io::Error::other(error.to_string()))
}
