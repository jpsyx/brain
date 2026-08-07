//! `tasks doctor`: validate selected-workspace feature and agent health.
//! wired up.
//!
//! Reports on the selected workspace's state DB, frontend lifecycle
//! integrations, rclone, sync state, and centralized feature requirements.
//!
//! A missing frontend hook means that frontend cannot record the brain panel's
//! sessions for resume.
//!
//! Rclone/sync is informational when sync is off; it does not affect
//! `Diagnosis::is_ok`.
//!
//! Output is structured (one line per check) so it scans at a glance.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Default, Clone)]
pub struct Diagnosis {
    pub db_path: PathBuf,
    pub db_present: bool,
    pub db_schema_ok: bool,
    pub settings_path: PathBuf,
    pub hook_installed: bool,
    pub hook_command: Option<String>,
    pub codex_hooks_path: PathBuf,
    pub claude_hook_installed: bool,
    pub codex_hook_installed: bool,
    pub opencode_plugin_installed: bool,
    pub rclone_version: Option<String>,
    pub sync_configured: bool,
}

impl Diagnosis {
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        self.db_present
            && self.db_schema_ok
            && self.claude_hook_installed
            && self.codex_hook_installed
            && self.opencode_plugin_installed
    }
}

/// Run all checks. Pure function over paths so tests can point it at
/// a temp dir; the binary entry point passes the selected workspace's
/// UUID-scoped state DB and `.claude` directory.
#[must_use]
pub fn run_doctor(db_path: &Path, settings_dir: &Path, sync_configured: bool) -> Diagnosis {
    run_doctor_with_frontends(
        db_path,
        settings_dir,
        Path::new(".codex/hooks.json"),
        sync_configured,
    )
}

/// Run the Claude/Codex-parity checks against explicit read-only paths.
#[must_use]
pub fn run_doctor_with_frontends(
    db_path: &Path,
    settings_dir: &Path,
    codex_hooks_path: &Path,
    sync_configured: bool,
) -> Diagnosis {
    crate::logging::log(format!(
        "doctor start db={} settings_dir={}",
        db_path.display(),
        settings_dir.display()
    ));
    let mut diag = Diagnosis {
        db_path: db_path.to_path_buf(),
        db_schema_ok: true, // vacuous when DB is missing
        settings_path: settings_dir.join("settings.json"),
        codex_hooks_path: codex_hooks_path.to_path_buf(),
        ..Default::default()
    };
    crate::logging::log("doctor check state db");
    diag.db_present = db_path.is_file();
    if diag.db_present {
        diag.db_schema_ok = check_db_schema(db_path).is_ok();
    }
    crate::logging::log(format!(
        "doctor state db present={} schema_ok={}",
        diag.db_present, diag.db_schema_ok
    ));
    crate::logging::log(format!(
        "doctor check SessionStart hook {}",
        diag.settings_path.display()
    ));
    if let Some(cmd) = find_session_start_hook(&diag.settings_path) {
        diag.hook_installed = true;
        diag.claude_hook_installed = true;
        diag.hook_command = Some(cmd);
    }
    diag.codex_hook_installed = find_session_start_hook(&diag.codex_hooks_path).is_some();
    let workspace_root = settings_dir.parent().unwrap_or_else(|| Path::new("."));
    diag.opencode_plugin_installed = workspace_root.join(".opencode/plugins/brain.js").is_file();
    crate::logging::log(format!(
        "doctor frontend integrations claude={} codex={} opencode={}",
        diag.claude_hook_installed, diag.codex_hook_installed, diag.opencode_plugin_installed
    ));
    crate::logging::log("doctor probe rclone");
    diag.rclone_version = detect_rclone_version();
    crate::logging::log(format!("doctor rclone version={:?}", diag.rclone_version));
    crate::logging::log("doctor load sync config");
    diag.sync_configured = sync_configured;
    crate::logging::log(format!("doctor sync configured={}", diag.sync_configured));
    diag
}

#[must_use]
pub fn format_doctor_plan(
    db_path: &Path,
    settings_path: &Path,
    theme: crate::theme::Theme,
) -> String {
    format!(
        "{}\n  {} {}\n  {} {}\n  {} {}\n  {} {}",
        theme.heading("Checking brain task environment"),
        theme.muted("state DB:"),
        theme.value(&db_path.display().to_string()),
        theme.muted("SessionStart hook:"),
        theme.value(&settings_path.display().to_string()),
        theme.muted("rclone:"),
        "probing PATH",
        theme.muted("sync config:"),
        "reading brain env",
    )
}

/// Detect `rclone` on `PATH` by running `rclone version` and parsing the
/// first line's version token (`rclone v1.74.2` -> `1.74.2`). `None` when
/// the binary is missing or its output doesn't match the expected shape.
fn detect_rclone_version() -> Option<String> {
    let out = Command::new("rclone")
        .args(["--config", "/dev/null", "version"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first_line = text.lines().next()?;
    let token = first_line.split_whitespace().nth(1)?;
    Some(token.strip_prefix('v').unwrap_or(token).to_owned())
}

fn check_db_schema(path: &Path) -> anyhow::Result<()> {
    // Smoke query: the migration we shipped creates this table. If it's
    // absent the DB is stale or corrupted.
    let conn = rusqlite::Connection::open_with_flags(
        immutable_sqlite_uri(path),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='brain_sessions'",
        [],
        |_| Ok(()),
    )?;
    Ok(())
}

fn immutable_sqlite_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            uri.push(char::from(*byte));
        } else {
            write!(uri, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    uri.push_str("?immutable=1");
    uri
}

/// Walk one frontend settings file for Brain's deployed SessionStart hook.
/// Returns the command on hit. Lenient on JSON shape: any unexpected structure
/// returns `None`.
fn find_session_start_hook(settings_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(settings_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let entries = val.get("hooks")?.get("SessionStart")?.as_array()?;
    for entry in entries {
        let hooks = entry.get("hooks").and_then(|h| h.as_array());
        let Some(hooks) = hooks else { continue };
        for hook in hooks {
            let cmd = hook.get("command").and_then(|c| c.as_str());
            if let Some(cmd) = cmd {
                if cmd.ends_with(".claude/brain-hooks/claude_session_start_hook.py") {
                    return Some(cmd.to_owned());
                }
            }
        }
    }
    None
}

/// One-line rclone/sync health summary for `brain tasks doctor`.
#[must_use]
pub fn sync_line(
    rclone_version: Option<&str>,
    configured: bool,
    theme: crate::theme::Theme,
) -> String {
    let rclone = rclone_version.map_or_else(
        || theme.error("rclone ✗ not installed"),
        |v| theme.success(&format!("rclone ✓ {v}")),
    );
    let sync = if configured {
        theme.success("sync configured")
    } else {
        theme.muted("sync off")
    };
    format!("{rclone} · {sync}")
}

/// Render a human-readable report to stdout. Returns 0 on full
/// health, 1 otherwise — useful as a `--ci` exit code.
#[must_use]
pub fn print_report(diag: &Diagnosis) -> i32 {
    let ok = |b: bool| if b { "✓" } else { "✗" };
    println!("tasks doctor");
    println!(
        "  {} state DB: {}",
        ok(diag.db_present),
        diag.db_path.display()
    );
    if diag.db_present {
        println!("  {} state DB schema", ok(diag.db_schema_ok));
    } else {
        println!("    (will be created on first tasks-shell run)");
    }
    println!(
        "  {} SessionStart hook in {}",
        ok(diag.hook_installed),
        diag.settings_path.display()
    );
    if let Some(cmd) = &diag.hook_command {
        println!("    → {cmd}");
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
    let _ = writeln!(
        output,
        "    {} Claude SessionStart: {}",
        mark(diag.claude_hook_installed),
        health(diag.claude_hook_installed, theme)
    );
    let _ = writeln!(
        output,
        "    {} Codex SessionStart: {}",
        mark(diag.codex_hook_installed),
        health(diag.codex_hook_installed, theme)
    );
    let _ = writeln!(
        output,
        "    {} OpenCode Brain plugin: {}",
        mark(diag.opencode_plugin_installed),
        health(diag.opencode_plugin_installed, theme)
    );
    if !diag.claude_hook_installed || !diag.codex_hook_installed || !diag.opencode_plugin_installed
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

const fn mark(ok: bool) -> &'static str {
    if ok { "✓" } else { "✗" }
}

fn health(ok: bool, theme: crate::theme::Theme) -> String {
    if ok {
        theme.success("ready")
    } else {
        theme.error("needs repair")
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests;
