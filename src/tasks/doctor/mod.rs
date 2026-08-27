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

mod frontend;
mod render;

pub use frontend::{FrontendCompatibility, FrontendHealth};
pub use render::{format_workspace_report, print_report, print_workspace_report, sync_line};

#[derive(Debug, Default, Clone)]
pub struct Diagnosis {
    pub db_path: PathBuf,
    pub db_present: bool,
    pub db_schema_ok: bool,
    pub settings_path: PathBuf,
    pub hook_command: Option<String>,
    pub frontend_health: Vec<FrontendHealth>,
    frontend_compatibility: Vec<FrontendCompatibility>,
    pub rclone_version: Option<String>,
    pub sync_configured: bool,
}

impl Diagnosis {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        let required_compatibility = crate::agent::registrations()
            .iter()
            .filter(|registration| registration.requires_compatibility_probe())
            .collect::<Vec<_>>();
        self.db_present
            && self.db_schema_ok
            && self.frontend_health.len() == crate::agent::AgentKind::ALL.len()
            && all_frontends_ready(&self.frontend_health)
            && self.frontend_compatibility.len() == required_compatibility.len()
            && required_compatibility.iter().all(|registration| {
                self.frontend_compatibility.iter().any(|compatibility| {
                    compatibility.kind() == registration.kind() && compatibility.is_ready()
                })
            })
    }

    /// Registry-ordered health for every functional frontend.
    #[must_use]
    pub fn frontend_health(&self) -> &[FrontendHealth] {
        &self.frontend_health
    }

    /// Whether the selected registered frontend's integration is ready.
    #[must_use]
    pub fn frontend_ready(&self, kind: crate::agent::AgentKind) -> bool {
        self.frontend_health
            .iter()
            .find(|health| health.kind() == kind)
            .is_some_and(FrontendHealth::is_ready)
    }

    /// Read-only executable compatibility rows collected by doctor.
    #[must_use]
    pub fn frontend_compatibility(&self) -> &[FrontendCompatibility] {
        &self.frontend_compatibility
    }

    pub(crate) fn record_frontend_compatibility(
        &mut self,
        kind: crate::agent::AgentKind,
        result: Result<Option<String>, crate::agent::AgentError>,
    ) {
        self.frontend_compatibility
            .retain(|health| health.kind() != kind);
        self.frontend_compatibility
            .push(FrontendCompatibility::from_result(kind, result));
    }
}

fn all_frontends_ready(health: &[FrontendHealth]) -> bool {
    health.iter().all(FrontendHealth::is_ready)
}

/// Run all checks. Pure function over paths so tests can point it at
/// a temp dir; the binary entry point passes the selected workspace's
/// UUID-scoped state DB and an injected legacy settings directory.
#[must_use]
pub fn run_doctor(
    db_path: &Path,
    settings_dir: &Path,
    sync_configured: bool,
    compatibility: &[(
        crate::agent::AgentKind,
        Result<Option<String>, crate::agent::AgentError>,
    )],
) -> Diagnosis {
    let workspace_root = settings_dir.parent().unwrap_or_else(|| Path::new("."));
    run_doctor_for_workspace(
        db_path,
        workspace_root,
        Path::new("."),
        sync_configured,
        compatibility,
    )
}

/// Compatibility wrapper over explicit legacy settings and hooks paths.
#[must_use]
pub fn run_doctor_with_frontends(
    db_path: &Path,
    settings_dir: &Path,
    codex_hooks_path: &Path,
    sync_configured: bool,
    compatibility: &[(
        crate::agent::AgentKind,
        Result<Option<String>, crate::agent::AgentError>,
    )],
) -> Diagnosis {
    let workspace_root = settings_dir.parent().unwrap_or_else(|| Path::new("."));
    let home = codex_hooks_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    run_doctor_for_workspace(
        db_path,
        workspace_root,
        home,
        sync_configured,
        compatibility,
    )
}

/// Run every registry-declared health check from a workspace root and home.
#[must_use]
pub fn run_doctor_for_workspace(
    db_path: &Path,
    workspace_root: &Path,
    home: &Path,
    sync_configured: bool,
    compatibility: &[(
        crate::agent::AgentKind,
        Result<Option<String>, crate::agent::AgentError>,
    )],
) -> Diagnosis {
    let primary_session = frontend::primary_session_check(workspace_root, home);
    let settings_path = primary_session
        .as_ref()
        .map(|(path, _, _)| path.clone())
        .unwrap_or_default();
    crate::logging::log(format!(
        "doctor start db={} workspace_root={}",
        db_path.display(),
        workspace_root.display()
    ));
    let mut diag = Diagnosis {
        db_path: db_path.to_path_buf(),
        db_schema_ok: true, // vacuous when DB is missing
        settings_path,
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
    diag.frontend_health = frontend::inspect(workspace_root, home);
    for (kind, result) in compatibility {
        diag.record_frontend_compatibility(*kind, result.clone());
    }
    diag.hook_command = primary_session
        .and_then(|(path, _, suffix)| frontend::session_start_command(&path, suffix));
    crate::logging::log(format!(
        "doctor frontend integrations claude={} codex={} opencode={}",
        diag.frontend_ready(crate::agent::AgentKind::Claude),
        diag.frontend_ready(crate::agent::AgentKind::Codex),
        diag.frontend_ready(crate::agent::AgentKind::OpenCode)
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
        "{}\n  {} {}\n  {} {}\n  {} {}\n  {} {}\n  {} {}\n  {} {}",
        theme.heading("Checking brain task environment"),
        theme.muted("state DB:"),
        theme.value(&db_path.display().to_string()),
        theme.muted("SessionStart hook:"),
        theme.value(&settings_path.display().to_string()),
        theme.muted("Claude:"),
        "probing configured command",
        theme.muted("OpenCode:"),
        "probing configured command",
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

#[cfg(test)]
mod tests;
