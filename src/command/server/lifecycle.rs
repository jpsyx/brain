//! HTTP server lifecycle command dispatch.

use anyhow::Result;

pub fn run_server(args: &crate::cli::ServerArgs) -> Result<()> {
    use crate::cli::ServerAction;
    match &args.action {
        ServerAction::Status => {
            crate::logging::log("server status");
            crate::server::lifecycle::status()
        }
        ServerAction::Logs => {
            crate::logging::log("server logs");
            crate::server::lifecycle::logs()
        }
        ServerAction::Run {
            generation,
            port,
            background,
        } => {
            crate::logging::log(format!("server run generation={generation} port={port}"));
            if *background {
                crate::server::run_background(*generation, *port)
            } else {
                crate::server::run(*generation, *port)
            }
        }
    }
}
