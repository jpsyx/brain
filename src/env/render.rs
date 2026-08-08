//! Rendering the `brain env` breakdown: the machine-global block, one block per
//! registered workspace, and the variable legend. Pure given a [`Theme`], the
//! same split `workspace list` and the `config list` table use.

use std::fmt::Write as _;

use super::breakdown::Breakdown;
use crate::settings::Resolved;
use crate::theme::Theme;

const UNSET: &str = "(unset)";
const EMPTY: &str = "(empty)";
const NONE: &str = "(none)";
const ROW_INDENT: &str = "    ";
const NESTED_NOTE: &str = "Dotted rows are nested values inside that workspace's own env object.";

/// The `brain env` view for this machine, themed for the active terminal. Thin
/// IO shell: reads the registry, then defers to [`format_breakdown`].
#[must_use]
pub fn render_breakdown(command: &crate::workspace::CommandContext) -> String {
    format_breakdown(&super::breakdown::collect(command), Theme::active())
}

/// The whole `brain env` view as one printable string.
#[must_use]
fn format_breakdown(breakdown: &Breakdown, theme: Theme) -> String {
    let width = name_width(breakdown);
    let mut out = String::new();
    let _ = writeln!(out, "{}\n", theme.heading("Brain environment"));
    let _ = writeln!(
        out,
        "{} {}\n",
        theme.muted("registry:"),
        theme.value(&breakdown.registry_path.display().to_string())
    );

    let _ = writeln!(out, "{}\n", theme.heading("Global"));
    if breakdown.global.is_empty() {
        let _ = writeln!(out, "{ROW_INDENT}{}", theme.muted(NONE));
    }
    for row in &breakdown.global {
        let _ = writeln!(
            out,
            "{}",
            data_line(&row.name, &value_cell(row, theme), width, theme)
        );
    }

    let _ = writeln!(out, "\n{}\n", theme.heading("Workspaces"));
    if breakdown.workspaces.is_empty() {
        let _ = writeln!(out, "{ROW_INDENT}{}", theme.muted(NONE));
    }
    for (index, workspace) in breakdown.workspaces.iter().enumerate() {
        if index > 0 {
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "{}", workspace_header(workspace, theme));
        for row in &workspace.rows {
            let _ = writeln!(
                out,
                "{}",
                data_line(&row.name, &value_cell(row, theme), width, theme)
            );
        }
    }

    let _ = writeln!(out, "\n{}\n", theme.heading("Variables"));
    for doc in &breakdown.variables {
        let _ = writeln!(
            out,
            "{}",
            data_line(&doc.name, &theme.muted(&doc.description), width, theme)
        );
    }
    let _ = writeln!(out, "\n{ROW_INDENT}{}", theme.muted(NESTED_NOTE));
    out
}

/// `* name (default, selected)`, mirroring the marker style `workspace list` uses.
fn workspace_header(workspace: &super::breakdown::WorkspaceEnv, theme: Theme) -> String {
    let marker = if workspace.is_default { "*" } else { " " };
    let mut labels = Vec::new();
    if workspace.is_default {
        labels.push("default");
    }
    if workspace.is_selected {
        labels.push("selected");
    }
    let suffix = if labels.is_empty() {
        String::new()
    } else {
        format!(" {}", theme.muted(&format!("({})", labels.join(", "))))
    };
    format!(
        "{} {}{suffix}",
        theme.success(marker),
        theme.accent(&workspace.name)
    )
}

/// One indented `name  value` line, the name padded to the shared column width.
fn data_line(name: &str, painted_value: &str, width: usize, theme: Theme) -> String {
    format!(
        "{ROW_INDENT}{}  {painted_value}",
        theme.accent(&format!("{name:<width$}"))
    )
}

/// The shared name-column width, so the global block, every workspace block, and
/// the legend line up as one grid.
fn name_width(breakdown: &Breakdown) -> usize {
    breakdown
        .global
        .iter()
        .chain(breakdown.workspaces.iter().flat_map(|w| w.rows.iter()))
        .map(|row| row.name.len())
        .chain(breakdown.variables.iter().map(|doc| doc.name.len()))
        .max()
        .unwrap_or(0)
}

/// A value cell distinguishes three states: absent, present-but-empty, and set.
/// An empty string would otherwise render as blank padding indistinguishable
/// from a rendering bug.
fn value_cell(row: &Resolved, theme: Theme) -> String {
    match row.value.as_deref() {
        None => theme.muted(UNSET),
        Some("") => theme.muted(EMPTY),
        Some(value) => theme.value(value),
    }
}

#[cfg(test)]
mod tests;
