//! Deterministic workspace-list rendering.

use std::fmt::Write;

use crate::access::AccessMode;
use crate::theme::Theme;
use crate::workspace::MachineRegistry;

struct WorkspaceListRow {
    name: String,
    is_default: bool,
    root: String,
    aliases: String,
    local_user: Option<String>,
    receiver_enabled: bool,
    access_mode: Option<AccessMode>,
}

pub(super) fn print(
    registry: &MachineRegistry,
    context: Option<&crate::workspace::CommandContext>,
    explicit_workspace: bool,
    theme: Theme,
) {
    let rows = collect_rows(registry);
    print!("{}", format_rows(&rows, theme));
    let Some(context) = context else {
        return;
    };
    // `-w` asks about one workspace; a bare list is a machine-wide inventory, so
    // reporting only the selected workspace's health would quietly hide every
    // peer's — including the ones a user is most likely to have forgotten.
    if explicit_workspace {
        print!("\n{}", requirements_block(context, theme));
        return;
    }
    for (name, record) in &registry.workspaces {
        if record.workspace_id == context.workspace.id() {
            print!("\n{}", requirements_block(context, theme));
            continue;
        }
        print!("\n{}", peer_requirements_block(name, record, theme));
    }
}

/// One workspace's requirements block, or a themed note when it cannot be read.
///
/// A half-configured peer must not take the whole inventory down with it: the
/// list is exactly where a user looks to discover that a workspace still needs
/// setup.
fn requirements_block(context: &crate::workspace::CommandContext, theme: Theme) -> String {
    crate::workspace::requirements(context).map_or_else(
        |error| unavailable_block(context.workspace.name().as_str(), &error.to_string(), theme),
        |requirements| {
            crate::workspace::format_requirements(context.workspace.name(), &requirements, theme)
        },
    )
}

/// Build a read-only context for a registered workspace this command did not
/// select, then report its requirements.
fn peer_requirements_block(
    name: &crate::workspace::WorkspaceName,
    record: &crate::workspace::WorkspaceRecord,
    theme: Theme,
) -> String {
    let Some(context) = crate::workspace::peer_context(name, record) else {
        return unavailable_block(name.as_str(), "workspace needs setup", theme);
    };
    requirements_block(&context, theme)
}

fn unavailable_block(name: &str, reason: &str, theme: Theme) -> String {
    format!(
        "{} {}\n  {}\n",
        theme.muted("Workspace"),
        theme.accent(name),
        theme.warning(&format!(
            "status unavailable: {reason} (fix: brain workspace repair -w {name})"
        )),
    )
}

fn collect_rows(registry: &MachineRegistry) -> Vec<WorkspaceListRow> {
    registry
        .workspaces
        .iter()
        .map(|(name, record)| WorkspaceListRow {
            name: name.to_string(),
            is_default: name == &registry.default_workspace,
            root: record.root.display().to_string(),
            aliases: if record.aliases.is_empty() {
                "none".to_owned()
            } else {
                record
                    .aliases
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            local_user: (!record.local_user_id.trim().is_empty())
                .then(|| record.local_user_id.clone()),
            receiver_enabled: record.receiver_enabled,
            access_mode: crate::config::Config::try_load_from_root(&record.root)
                .ok()
                .map(|config| config.access_mode),
        })
        .collect()
}

fn format_rows(rows: &[WorkspaceListRow], theme: Theme) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "{}\n", theme.heading("Workspaces"));
    for row in rows {
        let marker = if row.is_default { "*" } else { " " };
        let default = if row.is_default { " (default)" } else { "" };
        let _ = writeln!(
            output,
            "{} {}{}",
            theme.success(marker),
            theme.accent(&row.name),
            theme.muted(default)
        );
        let _ = writeln!(output, "    root: {}", theme.value(&row.root));
        let _ = writeln!(output, "    aliases: {}", theme.value(&row.aliases));
        let local_user = row.local_user.as_deref().map_or_else(
            || theme.warning("setup pending"),
            |value| theme.value(value),
        );
        let _ = writeln!(output, "    local user: {local_user}");
        let receiver = if row.receiver_enabled {
            theme.success("enabled")
        } else {
            theme.muted("disabled")
        };
        let _ = writeln!(output, "    receiver: {receiver}");
        if let Some(access_mode) = row.access_mode {
            for line in crate::access::render_access_status(access_mode, theme).lines() {
                let _ = writeln!(output, "    {line}");
            }
        } else {
            let _ = writeln!(output, "    Access mode  {}", theme.warning("incomplete"));
        }
    }
    output
}
