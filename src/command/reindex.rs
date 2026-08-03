//! Reindex command handler.

pub fn run(
    args: &crate::cli::ReindexArgs,
    context: &crate::workspace::CommandContext,
) -> anyhow::Result<()> {
    crate::reindex::run(
        &context.workspace,
        args.projects,
        args.resources,
        args.tasks,
    )
}
