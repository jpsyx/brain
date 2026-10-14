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
    #[arg(long)]
    pub installer_refresh_protocol: Option<u32>,
}

/// Installer-only target rendering and publication.
#[derive(Args, Debug)]
pub struct RefreshWorkspaceSkillsArgs {
    #[arg(long)]
    pub target_binary: std::path::PathBuf,
    #[arg(long)]
    pub target_version: String,
    #[arg(long, value_enum)]
    pub phase: RefreshPhase,
    #[arg(long)]
    pub expected_target_sha256: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RefreshPhase {
    Preflight,
    Apply,
}
