//! Human-readable doctor output.

use std::{fmt::Write as _, path::Path};

use super::Diagnosis;

/// One-line rclone/sync health summary for `brain tasks doctor`.
#[must_use]
pub fn sync_line(
    rclone_version: Option<&str>,
    configured: bool,
    theme: crate::theme::Theme,
) -> String {
    let rclone = rclone_version.map_or_else(
        || theme.error("rclone ✗ not installed"),
        |version| theme.success(&format!("rclone ✓ {version}")),
    );
    let sync = if configured {
        theme.success("sync configured")
    } else {
        theme.muted("sync off")
    };
    format!("{rclone} · {sync}")
}

/// Render a human-readable report to stdout. Returns 0 on full health.
#[must_use]
pub fn print_report(diag: &Diagnosis) -> i32 {
    let mark = |ready: bool| if ready { "✓" } else { "✗" };
    println!("tasks doctor");
    println!(
        "  {} state DB: {}",
        mark(diag.db_present),
        diag.db_path.display()
    );
    if diag.db_present {
        println!("  {} state DB schema", mark(diag.db_schema_ok));
    } else {
        println!("    (will be created on first tasks-shell run)");
    }
    println!(
        "  {} SessionStart hook in {}",
        mark(diag.frontend_ready(crate::agent::AgentKind::Claude)),
        diag.settings_path.display()
    );
    if let Some(command) = &diag.hook_command {
        println!("    → {command}");
    } else {
        println!(
            "    install with: {}/scripts/install_hook.sh",
            env!("CARGO_MANIFEST_DIR")
        );
    }
    println!(
        "  {}",
        sync_line(
            diag.rclone_version.as_deref(),
            diag.sync_configured,
            crate::theme::Theme::active()
        )
    );
    i32::from(!diag.is_ok())
}

/// Render the themed doctor report and centralized feature matrix.
#[must_use]
pub fn format_workspace_report(
    diag: &Diagnosis,
    workspace: &crate::workspace::WorkspaceName,
    workspace_root: &Path,
    requirements: &[crate::workspace::Requirement],
    theme: crate::theme::Theme,
) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "{} {}",
        theme.heading("Workspace"),
        theme.accent(workspace.as_str())
    );
    let _ = writeln!(output, "  {}", theme.heading("Agent sessions"));
    let _ = writeln!(
        output,
        "    {} state database: {}",
        mark(diag.db_present && diag.db_schema_ok),
        health(diag.db_present && diag.db_schema_ok, theme)
    );
    append_frontend_health(&mut output, diag, workspace_root, theme);
    let _ = writeln!(output, "  {}", theme.heading("Tools"));
    let _ = writeln!(
        output,
        "    {}",
        sync_line(diag.rclone_version.as_deref(), diag.sync_configured, theme)
    );
    output.push('\n');
    output.push_str(&crate::workspace::format_requirements(
        workspace,
        requirements,
        theme,
    ));
    output
}

fn append_frontend_health(
    output: &mut String,
    diag: &Diagnosis,
    workspace_root: &Path,
    theme: crate::theme::Theme,
) {
    for registration in crate::agent::registrations() {
        let frontend = diag
            .frontend_health()
            .iter()
            .find(|health| health.kind() == registration.kind());
        for descriptor in registration.health_checks() {
            let ready = frontend.is_some_and(|health| health.check_ready(descriptor.label()));
            let _ = writeln!(
                output,
                "    {} {} {}: {}",
                mark(ready),
                registration.label(),
                descriptor.label(),
                health(ready, theme)
            );
        }
    }
    for compatibility in diag.frontend_compatibility() {
        let ready = compatibility.is_ready();
        let _ = writeln!(
            output,
            "    {} {} compatibility: {} ({})",
            mark(ready),
            compatibility.kind().label(),
            health(ready, theme),
            theme.muted(compatibility.detail())
        );
    }
    if diag
        .frontend_health()
        .iter()
        .any(|health| !health.is_ready())
        || diag.frontend_health().len() != crate::agent::AgentKind::ALL.len()
    {
        let installer = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh");
        let _ = writeln!(
            output,
            "      {} {}",
            theme.muted("fix:"),
            theme.accent(&format!(
                "{} {}",
                shell_quote(&installer),
                shell_quote(workspace_root)
            ))
        );
    }
}

/// Print the complete selected-workspace doctor report.
#[must_use]
pub fn print_workspace_report(
    diag: &Diagnosis,
    workspace: &crate::workspace::WorkspaceName,
    workspace_root: &Path,
    requirements: &[crate::workspace::Requirement],
) -> i32 {
    print!(
        "{}",
        format_workspace_report(
            diag,
            workspace,
            workspace_root,
            requirements,
            crate::theme::Theme::active(),
        )
    );
    i32::from(!diag.is_ok())
}

const fn mark(ready: bool) -> &'static str {
    if ready { "✓" } else { "✗" }
}

fn health(ready: bool, theme: crate::theme::Theme) -> String {
    if ready {
        theme.success("ready")
    } else {
        theme.error("needs repair")
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}
