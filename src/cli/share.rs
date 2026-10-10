use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct ShareArgs {
    /// File or directory to share. Relative paths start at the current directory.
    pub path: Option<PathBuf>,
    /// Link lifetime (default: 7 days). Quote spaces, e.g. --expires "1 hour".
    #[arg(long, value_name = "DURATION")]
    pub expires: Option<String>,
    /// Print the download URL, path, and expiration as JSON.
    #[arg(long)]
    pub json: bool,
}
