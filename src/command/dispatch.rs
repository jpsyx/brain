//! Top-level command routing glue.

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Cmd};
use crate::tasks::cli::Cli as TasksCli;
use crate::workspace::{
    BootstrapContext, BootstrapPolicy, CommandContext, Invocation, RegistryStore, bootstrap_policy,
    invocation_for,
};

#[derive(Debug, Clone, Copy)]
enum DispatchCapability<'a> {
    None,
    Registry(&'a RegistryStore),
    Ready(&'a CommandContext),
}

fn capability_for(
    invocation: Invocation,
    bootstrap: &BootstrapContext,
) -> Result<DispatchCapability<'_>> {
    match (bootstrap_policy(invocation), bootstrap) {
        (BootstrapPolicy::None | BootstrapPolicy::InternalNoPrompt, BootstrapContext::None) => {
            Ok(DispatchCapability::None)
        }
        (BootstrapPolicy::RegistryOnly, BootstrapContext::RegistryOnly(store)) => {
            Ok(DispatchCapability::Registry(store))
        }
        (BootstrapPolicy::ReadyWorkspace, BootstrapContext::Ready(context)) => {
            Ok(DispatchCapability::Ready(context))
        }
        (BootstrapPolicy::RegistryOnly, _) => {
            anyhow::bail!("internal command dispatch expected a workspace registry")
        }
        (BootstrapPolicy::ReadyWorkspace, _) => {
            anyhow::bail!("internal command dispatch expected a ready workspace")
        }
        (BootstrapPolicy::None | BootstrapPolicy::InternalNoPrompt, _) => {
            anyhow::bail!("internal command dispatch expected no workspace capability")
        }
    }
}

pub fn run(
    mut cli: Cli,
    agent_kind: crate::session::AgentKind,
    bootstrap: &BootstrapContext,
) -> Result<()> {
    validate_agent_kind(agent_kind)?;
    let capability = capability_for(invocation_for(&cli), bootstrap)?;
    if let Some(Cmd::Workspace(args)) = &cli.command {
        crate::logging::log("dispatch workspace");
        return match capability {
            DispatchCapability::Registry(store) => {
                super::workspace::run_registry_only(args, cli.brain.as_deref(), store)
            }
            DispatchCapability::Ready(context) => super::workspace::run_ready(args, context),
            DispatchCapability::None => {
                anyhow::bail!("internal workspace command dispatch expected a workspace capability")
            }
        };
    }
    if let Some(Cmd::User(args)) = &cli.command {
        crate::logging::log("dispatch user");
        return match capability {
            DispatchCapability::Registry(store) => {
                super::users::run(args, cli.brain.as_deref(), store)
            }
            DispatchCapability::None | DispatchCapability::Ready(_) => {
                anyhow::bail!("internal user command dispatch expected a workspace registry")
            }
        };
    }
    if let Some(Cmd::Server(args)) = &cli.command {
        crate::logging::log("dispatch server");
        return super::server::run_server(args);
    }
    let context = ready_context(capability)?;
    if let Some(Cmd::Config(args)) = &cli.command {
        crate::logging::log("dispatch config");
        return super::configuration::run_config(args, context);
    }
    if let Some(Cmd::Env(args)) = &cli.command {
        crate::logging::log("dispatch env");
        return super::configuration::run_env(args, context);
    }
    if let Some(Cmd::Sync(args)) = &cli.command {
        crate::logging::log("dispatch sync");
        return super::sync::run(args, context);
    }
    if let Some(Cmd::Personalize(args)) = &cli.command {
        crate::logging::log("dispatch personalize");
        return super::configuration::run_personalize(args, context);
    }
    if let Some(Cmd::Skills(args)) = &cli.command {
        crate::logging::log("dispatch skills");
        return super::configuration::run_skills(args, context);
    }
    if let Some(Cmd::Receiver(args)) = &cli.command {
        crate::logging::log("dispatch receiver");
        return super::server::run_receiver(args, context);
    }
    if let Some(Cmd::Habits(args)) = &cli.command {
        match &args.action {
            None => crate::logging::log("dispatch habits"),
            Some(crate::cli::HabitsAction::Revive(_)) => {
                crate::logging::log("dispatch habits revive");
            }
            Some(crate::cli::HabitsAction::Skip(_)) => {
                crate::logging::log("dispatch habits skip");
            }
        }
        return super::server::run_habits(args, context);
    }
    if matches!(&cli.command, Some(Cmd::Check)) {
        crate::logging::log("dispatch check");
        let config = crate::sync::config::SyncConfig::load(context);
        crate::sync::check::run(context.workspace.paths(), &config, context.workspace.root());
        return Ok(());
    }
    if let Some(Cmd::Reindex(args)) = &cli.command {
        crate::logging::log("dispatch reindex");
        return super::reindex::run(args, context);
    }

    crate::settings::ensure_markdown_to_pdf(context);
    match cli.command {
        None => super::tasks::launch(
            TasksCli::parse_from(["brain"]),
            context,
            agent_kind,
            cli.with_receiver,
            cli.no_daily_triage_check,
        ),
        Some(Cmd::Tasks(ref mut args)) => {
            crate::logging::log("dispatch tasks");
            let agent_kind = if super::tasks::take_codex_flag(&mut args.rest) {
                crate::session::AgentKind::Codex
            } else {
                agent_kind
            };
            let rewritten = super::tasks::rewrite_mark_grammar(
                std::iter::once("brain tasks".to_owned())
                    .chain(std::mem::take(&mut args.rest))
                    .collect(),
            );
            super::tasks::launch(
                TasksCli::parse_from(rewritten),
                context,
                agent_kind,
                cli.with_receiver,
                cli.no_daily_triage_check,
            )
        }
        Some(Cmd::Version) => unreachable!("version exits before bootstrap"),
        Some(
            Cmd::Config(_)
            | Cmd::Env(_)
            | Cmd::Sync(_)
            | Cmd::Personalize(_)
            | Cmd::Skills(_)
            | Cmd::Server(_)
            | Cmd::Receiver(_)
            | Cmd::Habits(_)
            | Cmd::Check
            | Cmd::Reindex(_)
            | Cmd::Workspace(_)
            | Cmd::User(_),
        ) => unreachable!("short-lived command dispatched above"),
    }
}

/// Reject a known selection stub before workspace, TUI, hook, server, or PTY setup.
///
/// # Errors
///
/// Returns [`crate::agent::AgentError::UnsupportedFrontend`] for OpenCode.
pub fn validate_agent_kind(
    agent_kind: crate::session::AgentKind,
) -> Result<(), crate::agent::AgentError> {
    match agent_kind {
        crate::session::AgentKind::Claude | crate::session::AgentKind::Codex => Ok(()),
        crate::session::AgentKind::OpenCode => Err(crate::agent::AgentError::UnsupportedFrontend(
            crate::session::AgentKind::OpenCode,
        )),
    }
}

fn ready_context(capability: DispatchCapability<'_>) -> Result<&CommandContext> {
    let DispatchCapability::Ready(context) = capability else {
        anyhow::bail!("internal command dispatch expected a ready workspace");
    };
    Ok(context)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::capability_for;
    use crate::workspace::{
        BootstrapContext, CommandContext, Invocation, RegistryStore, WorkspaceContext, WorkspaceId,
        WorkspaceName,
    };

    #[test]
    fn dispatch_rejects_bootstrap_capabilities_for_the_wrong_policy() {
        let store = RegistryStore::from_path(std::path::PathBuf::from("/tmp/registry.json"));
        let ready = BootstrapContext::Ready(CommandContext::for_test(
            Arc::new(
                WorkspaceContext::new(
                    Path::new("/home/tester"),
                    WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
                    WorkspaceName::parse("brain").unwrap(),
                    Path::new("/brains/brain"),
                    "tester",
                    Path::new("/"),
                )
                .unwrap(),
            ),
            RegistryStore::from_path(PathBuf::from("/tmp/ready.json")),
            "tester",
        ));

        let ordinary_error =
            capability_for(Invocation::Config, &BootstrapContext::RegistryOnly(store)).unwrap_err();
        let registry_error =
            capability_for(Invocation::WorkspaceCreate, &BootstrapContext::None).unwrap_err();
        let ready_for_registry_error =
            capability_for(Invocation::WorkspaceCreate, &ready).unwrap_err();
        let none_error = capability_for(
            Invocation::Help,
            &BootstrapContext::RegistryOnly(RegistryStore::from_path(PathBuf::from(
                "/tmp/help.json",
            ))),
        )
        .unwrap_err();

        assert!(ordinary_error.to_string().contains("ready workspace"));
        assert!(registry_error.to_string().contains("workspace registry"));
        assert!(
            ready_for_registry_error
                .to_string()
                .contains("workspace registry")
        );
        assert!(none_error.to_string().contains("no workspace capability"));
    }
}
