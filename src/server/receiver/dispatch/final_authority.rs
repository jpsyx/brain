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
    control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .revalidate_workspace_route(workspace, clock())
        .map_err(|error| std::io::Error::other(error.to_string()))
}

pub(super) fn commit_admission(
    control: &std::sync::Mutex<crate::server::control::ControlServer>,
    workspace: &crate::server::workspace_route::ResolvedWorkspaceRoute,
    admission: &crate::server::receiver::admission::ReceiverAdmission,
    clock: &impl Fn() -> std::time::Instant,
    #[cfg(test)] after_intent_reload: Option<&(dyn Fn() + Send + Sync)>,
    #[cfg(test)] after_commit: Option<
        &(dyn Fn(&crate::server::receiver::admission::ReceiverAdmission) + Send + Sync),
    >,
) -> std::io::Result<()> {
    workspace
        .revalidate_receiver_intent()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    #[cfg(test)]
    if let Some(hook) = after_intent_reload {
        hook();
    }
    let server = control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = clock();
    server
        .revalidate_workspace_route(workspace, now)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if admission.workspace_id() != workspace.lease().workspace_id
        || admission.lease_id() != workspace.lease().lease_id
    {
        return Err(std::io::Error::other(
            "receiver admission no longer matches the live workspace lease",
        ));
    }
    admission.commit()?;
    #[cfg(test)]
    if let Some(hook) = after_commit {
        hook(admission);
    }
    drop(server);
    Ok(())
}
