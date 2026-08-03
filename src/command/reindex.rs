//! Reindex command handler.

pub fn run(
    args: &crate::cli::ReindexArgs,
    _context: &crate::workspace::CommandContext,
) -> anyhow::Result<()> {
    crate::reindex::run(args.projects, args.resources, args.tasks)
}
