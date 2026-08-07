//! Habits web command dispatch.

use anyhow::Result;

pub fn run_habits(
    args: &crate::cli::HabitsArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    match &args.action {
        None => open_habits(context),
        Some(crate::cli::HabitsAction::Kill) => kill_habits(context),
        Some(crate::cli::HabitsAction::Revive(revive)) => {
            crate::tasks::revive::run(&context.workspace, &revive.query.join(" "), &context.actor)
        }
        Some(crate::cli::HabitsAction::Skip(skip)) => crate::tasks::skip::run(
            &context.workspace,
            &skip.id,
            skip.until.as_deref(),
            &context.actor,
        ),
        Some(crate::cli::HabitsAction::CompleteManagedTriage(args)) => {
            crate::tasks::triage_habits::complete_managed_triage_cli(
                &context.workspace,
                args.kind.into(),
            )
        }
    }
}

fn kill_habits(context: &crate::workspace::CommandContext) -> Result<()> {
    let client = crate::server::lifecycle::ServerClient::default();
    let (record, snapshot) = client.snapshot()?;
    if snapshot.live_leases > 0 {
        anyhow::bail!("habit server cannot be stopped while a brain TUI is open");
    }
    let (_, capability) = client.workspace_local_route(context.workspace.id())?;
    let decision = client.unregister_generation(record.generation, capability)?;
    if decision != crate::server::lifecycle::ServerDecision::ShutdownNow {
        anyhow::bail!("habit server did not accept the stop request");
    }
    println!(
        "{}",
        crate::theme::Theme::active().success("Habit server stopped")
    );
    Ok(())
}

fn open_habits(context: &crate::workspace::CommandContext) -> Result<()> {
    let theme = crate::theme::Theme::active();
    let client = crate::server::lifecycle::ServerClient::default();
    let record = match client.snapshot() {
        Ok((_, snapshot)) if snapshot.live_leases > 0 => {
            anyhow::bail!("brain TUI is already running; close it before starting brain habits")
        }
        Ok(_) => anyhow::bail!("brain habits is already running"),
        Err(_) => {
            crate::logging::log("habits start background server");
            crate::server::lifecycle::connect_or_elect_background(&client)?
        }
    };
    let manifest = crate::workspace::WorkspaceManifest::load(
        context.workspace.root(),
        env!("CARGO_PKG_VERSION"),
    )?;
    let registration = crate::server::control::LeaseRegistration {
        generation: record.generation,
        lease_id: crate::server::lifecycle::LeaseId::new(),
        workspace_id: context.workspace.id(),
        canonical_name: context.workspace.name().to_string(),
        ingress_id: manifest.receiver_ingress_id().into(),
        tui_pid: 0,
        resolved_root: context.workspace.root().to_path_buf(),
        job_socket: std::path::PathBuf::new(),
    };
    client.start_background(&registration)?;
    let port = client.connect_existing()?.port;
    let (ingress, capability) = client.workspace_local_route(context.workspace.id())?;
    let target = crate::server::habits_url(port, ingress, capability);
    crate::logging::log(format!("habits open {target}"));
    println!("{}", theme.info(&format!("Opening {target}")));
    crate::logging::log(format!("spawn open {target}"));
    let _ = std::process::Command::new("open").arg(&target).spawn();
    Ok(())
}
