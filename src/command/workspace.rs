//! Workspace registry command handler.

pub fn run_registry_only(
    args: &crate::cli::WorkspaceArgs,
    selector: Option<&str>,
    store: &crate::workspace::RegistryStore,
) -> anyhow::Result<()> {
    crate::workspace::command::run_registry_only(args, selector, store)
}

pub fn run_ready(
    args: &crate::cli::WorkspaceArgs,
    context: &crate::workspace::CommandContext,
) -> anyhow::Result<()> {
    crate::workspace::command::run_ready(args, context)
}
