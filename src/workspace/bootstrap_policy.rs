//! Pure command classification for workspace bootstrap.

/// One parsed Brain invocation, classified before any workspace IO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    Help,
    Version,
    AgentHook,
    InternalServer,
    WorkspaceCreate,
    WorkspaceAttach,
    WorkspaceRemove,
    WorkspaceRepair,
    WorkspaceMigrate,
    WorkspaceList,
    WorkspaceRename,
    WorkspaceAlias,
    WorkspaceDefault,
    User,
    Config,
    Env,
    Sync,
    SyncStatus,
    Check,
    Persona,
    Skills,
    Server,
    ServerStatus,
    Killall,
    Receiver,
    ReceiverStatus,
    Habits,
    Reindex,
    Tasks,
    TasksDoctor,
    Tui,
}

/// How much workspace bootstrap an invocation permits and requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapPolicy {
    None,
    InternalNoPrompt,
    RegistryOnly,
    ReadOnlyWorkspace,
    ReadyWorkspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegistryOnlyPromptOrder {
    BeforeMigration,
}

pub(super) const fn registry_only_prompt_order(
    invocation: Invocation,
) -> Option<RegistryOnlyPromptOrder> {
    match invocation {
        Invocation::WorkspaceCreate
        | Invocation::WorkspaceAttach
        | Invocation::WorkspaceRemove
        | Invocation::WorkspaceRepair
        | Invocation::User => Some(RegistryOnlyPromptOrder::BeforeMigration),
        Invocation::Help
        | Invocation::Version
        | Invocation::AgentHook
        | Invocation::InternalServer
        | Invocation::WorkspaceList
        | Invocation::WorkspaceRename
        | Invocation::WorkspaceAlias
        | Invocation::WorkspaceDefault
        | Invocation::WorkspaceMigrate
        | Invocation::Config
        | Invocation::Env
        | Invocation::Sync
        | Invocation::SyncStatus
        | Invocation::Check
        | Invocation::Persona
        | Invocation::Skills
        | Invocation::Server
        | Invocation::ServerStatus
        | Invocation::Killall
        | Invocation::Receiver
        | Invocation::ReceiverStatus
        | Invocation::Habits
        | Invocation::Reindex
        | Invocation::Tasks
        | Invocation::TasksDoctor
        | Invocation::Tui => None,
    }
}

/// Return the explicit bootstrap policy for an invocation.
#[must_use]
pub const fn bootstrap_policy(invocation: Invocation) -> BootstrapPolicy {
    match invocation {
        Invocation::Help
        | Invocation::Version
        | Invocation::Server
        | Invocation::ServerStatus
        | Invocation::Killall => BootstrapPolicy::None,
        Invocation::AgentHook | Invocation::InternalServer => BootstrapPolicy::InternalNoPrompt,
        Invocation::WorkspaceCreate
        | Invocation::WorkspaceAttach
        | Invocation::WorkspaceRemove
        | Invocation::WorkspaceRepair
        | Invocation::User => BootstrapPolicy::RegistryOnly,
        Invocation::WorkspaceList
        | Invocation::ReceiverStatus
        | Invocation::SyncStatus
        | Invocation::TasksDoctor => BootstrapPolicy::ReadOnlyWorkspace,
        Invocation::WorkspaceRename
        | Invocation::WorkspaceAlias
        | Invocation::WorkspaceDefault
        | Invocation::WorkspaceMigrate
        | Invocation::Config
        | Invocation::Env
        | Invocation::Sync
        | Invocation::Check
        | Invocation::Persona
        | Invocation::Skills
        | Invocation::Receiver
        | Invocation::Habits
        | Invocation::Reindex
        | Invocation::Tasks
        | Invocation::Tui => BootstrapPolicy::ReadyWorkspace,
    }
}

/// Classify one already parsed CLI route without consulting workspace state.
#[must_use]
pub fn invocation_for(cli: &crate::cli::Cli) -> Invocation {
    use crate::cli::{Cmd, ServerAction, WorkspaceAction};

    match &cli.command {
        None => Invocation::Tui,
        Some(Cmd::Version) => Invocation::Version,
        Some(Cmd::Workspace(args)) => match &args.action {
            WorkspaceAction::List => Invocation::WorkspaceList,
            WorkspaceAction::Create { .. } => Invocation::WorkspaceCreate,
            WorkspaceAction::Attach { .. } => Invocation::WorkspaceAttach,
            WorkspaceAction::Rename { .. } => Invocation::WorkspaceRename,
            WorkspaceAction::Alias(_) => Invocation::WorkspaceAlias,
            WorkspaceAction::Default { .. } => Invocation::WorkspaceDefault,
            WorkspaceAction::Remove { .. } => Invocation::WorkspaceRemove,
            WorkspaceAction::Repair { .. } => Invocation::WorkspaceRepair,
            WorkspaceAction::Migrate { .. } => Invocation::WorkspaceMigrate,
        },
        Some(Cmd::User(_)) => Invocation::User,
        Some(Cmd::Config(_)) => Invocation::Config,
        Some(Cmd::Env(_)) => Invocation::Env,
        Some(Cmd::Sync(args)) => {
            if matches!(args.action, Some(crate::cli::SyncAction::Status)) {
                Invocation::SyncStatus
            } else {
                Invocation::Sync
            }
        }
        Some(Cmd::Check) => Invocation::Check,
        Some(Cmd::Killall) => Invocation::Killall,
        Some(Cmd::Persona(_)) => Invocation::Persona,
        Some(Cmd::Skills(_)) => Invocation::Skills,
        Some(Cmd::Server(args)) => match args.action {
            ServerAction::Run { .. } => Invocation::InternalServer,
            ServerAction::Status => Invocation::ServerStatus,
            ServerAction::Logs => Invocation::Server,
        },
        Some(Cmd::Receiver(args)) => match args.action {
            crate::cli::ReceiverServerAction::Status => Invocation::ReceiverStatus,
            _ => Invocation::Receiver,
        },
        Some(Cmd::Habits(_)) => Invocation::Habits,
        Some(Cmd::Reindex(_)) => Invocation::Reindex,
        Some(Cmd::Tasks(args)) => {
            if tasks_doctor(&args.rest) {
                Invocation::TasksDoctor
            } else {
                Invocation::Tasks
            }
        }
    }
}

/// Whether a command must avoid every diagnostic and bootstrap write.
#[must_use]
pub fn is_read_only_status(cli: &crate::cli::Cli) -> bool {
    matches!(
        invocation_for(cli),
        Invocation::ServerStatus
            | Invocation::ReceiverStatus
            | Invocation::SyncStatus
            | Invocation::WorkspaceList
            | Invocation::TasksDoctor
    )
}

fn tasks_doctor(arguments: &[String]) -> bool {
    arguments
        .iter()
        .find(|argument| !matches!(argument.as_str(), "--codex" | "-cx"))
        .is_some_and(|argument| argument == "doctor")
}
