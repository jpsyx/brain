//! Habits web command dispatch.

use anyhow::Result;

pub fn run_habits(
    args: &crate::cli::HabitsArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    match &args.action {
        None => open_habits(context),
        Some(crate::cli::HabitsAction::Kill) => kill_habits(context),
        Some(crate::cli::HabitsAction::Revive(revive)) => crate::tasks::revive::run(
            &context.registry_store,
            &context.workspace,
            &revive.query.join(" "),
            &context.actor,
        ),
        Some(crate::cli::HabitsAction::Skip(skip)) => crate::tasks::skip::run(
            &context.registry_store,
            &context.workspace,
            &skip.id,
            skip.until.as_deref(),
            &context.actor,
        ),
        Some(crate::cli::HabitsAction::CompleteManagedTriage(args)) => {
            crate::tasks::triage_habits::complete_managed_triage_cli(
                &context.registry_store,
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
    // Opening a page is not a claim on the process. Whatever is already serving
    // — an open TUI's server or an earlier background one — serves this too;
    // only a machine with nothing running needs one elected. Refusing while a
    // TUI was open made the most common moment to want today's habits the one
    // moment the command would not work.
    let record = if let Ok((record, _)) = client.snapshot() {
        crate::logging::log("habits reuse running server");
        record
    } else {
        crate::logging::log("habits start background server");
        crate::server::lifecycle::connect_or_elect_background(&client)?
    };
    // A workspace is routable only once something registered a lease for it. An
    // open TUI for *this* workspace already did; anything else (no TUI, or a TUI
    // for a different workspace) needs a background lease of our own.
    let route = if let Ok(route) = client.workspace_local_route(context.workspace.id()) {
        route
    } else {
        {
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
            client.workspace_local_route(context.workspace.id())?
        }
    };
    let port = client.connect_existing()?.port;
    let (ingress, capability) = route;
    let target = crate::server::habits_url(port, ingress, capability);
    crate::logging::log(format!("habits open {target}"));
    println!("{}", theme.info(&format!("Opening {target}")));
    crate::logging::log(format!("spawn open {target}"));
    let _ = std::process::Command::new("open").arg(&target).spawn();
    Ok(())
}
