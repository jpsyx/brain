use clap::Args;

/// Installer-only version transition. Hidden from the public command surface.
#[derive(Args, Debug)]
pub struct InternalMigrationArgs {
    /// Version whose artifacts are currently installed.
    #[arg(long)]
    pub from_version: String,
    /// Version whose artifacts must be installed next.
    #[arg(long)]
    pub to_version: String,
}
