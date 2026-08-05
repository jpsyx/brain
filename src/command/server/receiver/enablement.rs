//! Exact-workspace receiver intent mutations and status reporting.

use anyhow::{Context, Result};

#[cfg(test)]
mod tests;

pub(crate) trait ReceiverIntentRefresher {
    fn refresh_enabled(&self, workspace_id: crate::workspace::WorkspaceId) -> Result<()>;
}

impl ReceiverIntentRefresher for crate::server::control::ServerClient {
    fn refresh_enabled(&self, workspace_id: crate::workspace::WorkspaceId) -> Result<()> {
        let Some(record) = crate::server::lifecycle::read_record(self.paths()) else {
            return Ok(());
        };
        self.refresh_enabled_generation(record.generation, workspace_id)
            .map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiverActionOutcome {
    enabled: bool,
    refresh_warning: Option<String>,
}

impl ReceiverActionOutcome {
    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn refresh_warning(&self) -> Option<&str> {
        self.refresh_warning.as_deref()
    }
}

pub(crate) fn apply_startup_receiver_flag(
    with_receiver: bool,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    if let Some(outcome) = apply_startup_receiver_flag_with(
        with_receiver,
        context,
        &crate::server::control::ServerClient::default(),
    )? {
        print_receiver_refresh_warning(&outcome);
    }
    Ok(())
}

pub(crate) fn apply_startup_receiver_flag_with(
    with_receiver: bool,
    context: &crate::workspace::CommandContext,
    refresher: &dyn ReceiverIntentRefresher,
) -> Result<Option<ReceiverActionOutcome>> {
    with_receiver
        .then(|| {
            apply_receiver_action_with(
                context,
                crate::workspace::ReceiverAction::WithReceiverFlag,
                refresher,
            )
        })
        .transpose()
}

pub(crate) fn apply_receiver_action_with(
    context: &crate::workspace::CommandContext,
    action: crate::workspace::ReceiverAction,
    refresher: &dyn ReceiverIntentRefresher,
) -> Result<ReceiverActionOutcome> {
    let enabled = context
        .registry_store
        .transition_receiver(context.workspace.name(), context.workspace.id(), action)
        .context("persisting receiver intent for the selected workspace")?;
    let refresh_warning = refresher
        .refresh_enabled(context.workspace.id())
        .err()
        .map(|error| format!("refreshing receiver intent in the live shared server: {error:#}"));
    Ok(ReceiverActionOutcome {
        enabled,
        refresh_warning,
    })
}

pub(crate) fn receiver_enabled(context: &crate::workspace::CommandContext) -> Result<bool> {
    let registry = crate::workspace::RegistryStore::load_from(context.registry_store.path())?;
    let selected = registry.select(Some(context.workspace.name().as_str()))?;
    if selected.record().workspace_id != context.workspace.id() {
        anyhow::bail!("selected workspace identity changed while reading receiver intent");
    }
    Ok(selected.record().receiver_enabled)
}

pub(super) fn print_receiver_change(outcome: &ReceiverActionOutcome) {
    let theme = crate::theme::Theme::active();
    let state = if outcome.enabled() {
        "enabled"
    } else {
        "disabled"
    };
    println!("{}", theme.success(&format!("Receiver {state}")));
    print_receiver_refresh_warning(outcome);
}

fn print_receiver_refresh_warning(outcome: &ReceiverActionOutcome) {
    let theme = crate::theme::Theme::active();
    if let Some(warning) = outcome.refresh_warning() {
        eprintln!("{}", theme.warning(&format!("Warning: {warning}")));
    }
}

pub(super) fn print_receiver_status(context: &crate::workspace::CommandContext) -> Result<()> {
    let status = read_receiver_status(context)?;
    print_receiver_status_snapshot(status);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReceiverStatus {
    pub(crate) enabled: bool,
    pub(crate) tui_live: bool,
    pub(crate) server_running: bool,
    pub(crate) accepting: bool,
}

pub(crate) const fn receiver_status(
    enabled: bool,
    server_running: bool,
    live_receiver_enabled: Option<bool>,
) -> ReceiverStatus {
    let tui_live = live_receiver_enabled.is_some();
    ReceiverStatus {
        enabled,
        tui_live,
        server_running,
        accepting: enabled && matches!(live_receiver_enabled, Some(true)),
    }
}

pub(crate) fn read_receiver_status(
    context: &crate::workspace::CommandContext,
) -> Result<ReceiverStatus> {
    let enabled = receiver_enabled(context)?;
    let client = crate::server::lifecycle::ServerClient::default();
    let server_running = client.connect_existing().is_ok();
    let live_receiver_enabled = server_running
        .then(|| client.workspace_receiver_enabled(context.workspace.id()))
        .transpose()
        .ok()
        .flatten()
        .flatten();
    Ok(receiver_status(
        enabled,
        server_running,
        live_receiver_enabled,
    ))
}

fn print_receiver_status_snapshot(status: ReceiverStatus) {
    let theme = crate::theme::Theme::active();
    println!(
        "{}  {}",
        theme.muted("Receiver"),
        theme.value(if status.enabled {
            "enabled"
        } else {
            "disabled"
        })
    );
    println!(
        "{}       {}",
        theme.muted("TUI"),
        theme.value(if status.tui_live { "live" } else { "not live" })
    );
    println!(
        "{}    {}",
        theme.muted("Server"),
        theme.value(if status.server_running {
            "running"
        } else {
            "not running"
        })
    );
    println!(
        "{} {}",
        theme.muted("Accepting"),
        theme.value(if status.accepting { "yes" } else { "no" })
    );
}
