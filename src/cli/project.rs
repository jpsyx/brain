//! `brain project` — the CLI surface for PARA project bookkeeping.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand, Debug)]
pub enum ProjectAction {
    /// Scaffold a project: the folder, its `.METADATA.json`, and a README.
    /// Deciding the namespace, the outcome slug, and the priority is yours;
    /// writing the record exactly is not.
    New(ProjectNewArgs),

    /// Change a project's title, status, priority, or due date. Only the
    /// fields you pass move, and the lookup is rebuilt afterwards.
    Set(ProjectSetArgs),

    /// Move a project into `archive/projects/`, keeping its folder name and
    /// repointing its record.
    Archive(ProjectSlugArgs),

    /// Describe a project, including whether every open task under it has been
    /// ignored long enough that it looks abandoned rather than finished.
    Show(ProjectShowArgs),
}

#[derive(Args, Debug)]
pub struct ProjectNewArgs {
    /// `<namespace>__<outcome-slug>`, lowercase kebab.
    pub slug: String,

    /// Human-readable title; becomes the README's H1.
    #[arg(long)]
    pub title: String,

    /// One of not-started, in-progress, blocked, extracting-ips, done.
    #[arg(long, default_value = "not-started")]
    pub status: String,

    /// One of p0–p4. Required: a project with no priority never gets triaged.
    #[arg(long)]
    pub priority: String,

    /// Absolute `YYYY-MM-DD`, or `none`.
    #[arg(long, default_value = "none")]
    pub due: String,

    /// One or two sentences for the README.
    #[arg(long, default_value = "")]
    pub description: String,
}

#[derive(Args, Debug)]
pub struct ProjectSetArgs {
    pub slug: String,

    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub priority: Option<String>,
    #[arg(long)]
    pub due: Option<String>,
}

#[derive(Args, Debug)]
pub struct ProjectSlugArgs {
    pub slug: String,
}

#[derive(Args, Debug)]
pub struct ProjectShowArgs {
    pub slug: String,

    /// Emit JSON instead of a themed summary.
    #[arg(long)]
    pub json: bool,
}
