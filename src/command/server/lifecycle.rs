//! HTTP server lifecycle command dispatch.

use anyhow::Result;

pub fn run_server(
    args: &crate::cli::ServerArgs,
    context: Option<&crate::workspace::CommandContext>,
) -> Result<()> {
    use crate::cli::ServerAction;
    match &args.action {
        ServerAction::Start => {
            let _context =
                context.ok_or_else(|| anyhow::anyhow!("server start needs a ready workspace"))?;
            crate::logging::log("server start");
            crate::server::lifecycle::start()
        }
        ServerAction::Status => {
            let _context =
                context.ok_or_else(|| anyhow::anyhow!("server status needs a ready workspace"))?;
            crate::logging::log("server status");
            crate::server::lifecycle::status()
        }
        ServerAction::Kill => {
            let _context =
                context.ok_or_else(|| anyhow::anyhow!("server kill needs a ready workspace"))?;
            crate::logging::log("server kill");
            crate::server::lifecycle::kill()
        }
        ServerAction::Run { port } => {
            crate::logging::log(format!("server run port={port}"));
            crate::server::run(*port)
        }
    }
}
