//! Reindex command handler.

pub fn run(
    args: &crate::cli::ReindexArgs,
    context: &crate::workspace::CommandContext,
) -> anyhow::Result<()> {
    crate::reindex::run(
        &context.workspace,
        &context.actor,
        args.projects,
        args.resources,
        args.tasks,
    )
}
