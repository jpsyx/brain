use std::path::PathBuf;

use clap::Args;

/// Open or copy a specific Brain run's diagnostic log.
#[derive(Args, Debug)]
#[command(after_help = "Examples:\n  brain logs /tmp/run.log\n  brain logs /tmp/run.log --copy")]
pub struct LogsArgs {
    /// Log file to open (prompts for a path when omitted).
    pub file: Option<PathBuf>,
    /// Copy the latest 2,000 lines (at most 256 KiB) instead of opening the file.
    #[arg(long)]
    pub copy: bool,
}
