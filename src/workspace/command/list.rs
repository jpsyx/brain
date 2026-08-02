//! Deterministic workspace-list rendering.

use std::fmt::Write;

use crate::theme::Theme;
use crate::workspace::MachineRegistry;

struct WorkspaceListRow {
    name: String,
    is_default: bool,
    root: String,
    aliases: String,
    local_user: Option<String>,
    receiver_enabled: bool,
    access_mode: Option<String>,
}

pub(super) fn print(registry: &MachineRegistry, theme: Theme) {
    let rows = collect_rows(registry);
    print!("{}", format_rows(&rows, theme));
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
            access_mode: read_access_mode(&record.root),
        })
        .collect()
}

fn read_access_mode(root: &std::path::Path) -> Option<String> {
    let body = std::fs::read_to_string(root.join(".config/config.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&body)
        .ok()?
        .get("access_mode")?
        .as_str()
        .map(str::to_owned)
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
        let access_mode = row.access_mode.as_deref().map_or_else(
            || theme.warning("setup pending"),
            |value| theme.value(value),
        );
        let _ = writeln!(output, "    access mode: {access_mode}");
    }
    output
}
