//! `tasks doctor` — validate that the brain panel's session tracking is
//! wired up.
//!
//! Reports on:
//! - State DB file presence + a smoke schema query.
//! - `~/brain/.claude/settings.json` has a SessionStart-hook entry pointing
//!   at our `claude_session_start_hook.py`.
//!
//! Failure here means the SessionStart hook never records the brain panel's
//! sessions, so the panel can't resume them — every open starts a fresh chat.
//! Output is structured (one line per check) so it scans at a glance.

use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Diagnosis {
    pub db_path: PathBuf,
    pub db_present: bool,
    pub db_schema_ok: bool,
    pub settings_path: PathBuf,
    pub hook_installed: bool,
    pub hook_command: Option<String>,
}

impl Diagnosis {
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        self.db_present && self.db_schema_ok && self.hook_installed
    }
}

/// Run all checks. Pure function over paths so tests can point it at
/// a temp dir; the binary entry point uses `Db::default_path` and
/// `~/brain/.claude` as the production locations.
#[must_use]
pub fn run_doctor(db_path: &Path, settings_dir: &Path) -> Diagnosis {
    let mut diag = Diagnosis {
        db_path: db_path.to_path_buf(),
        db_schema_ok: true, // vacuous when DB is missing
        settings_path: settings_dir.join("settings.json"),
        ..Default::default()
    };
    diag.db_present = db_path.is_file();
    if diag.db_present {
        diag.db_schema_ok = check_db_schema(db_path).is_ok();
    }
    if let Some(cmd) = find_session_start_hook(&diag.settings_path) {
        diag.hook_installed = true;
        diag.hook_command = Some(cmd);
    }
    diag
}

fn check_db_schema(path: &Path) -> anyhow::Result<()> {
    // Smoke query: the migration we shipped creates this table. If it's
    // absent the DB is stale or corrupted.
    let conn = rusqlite::Connection::open(path)?;
    conn.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='brain_sessions'",
        [],
        |_| Ok(()),
    )?;
    Ok(())
}

/// Walk `settings.json` looking for the merged shell's SessionStart-hook
/// entry. We match `brain/scripts/claude_session_start_hook.py` (the path is
/// under `~/scripts/rc/brain`) on the full command string. Returns the command
/// on hit. Lenient on JSON shape: any unexpected structure → None.
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
                if cmd.ends_with("brain/scripts/claude_session_start_hook.py") {
                    return Some(cmd.to_owned());
                }
            }
        }
    }
    None
}

/// Render a human-readable report to stdout. Returns 0 on full
/// health, 1 otherwise — useful as a `--ci` exit code.
#[must_use]
pub fn print_report(diag: &Diagnosis) -> i32 {
    let ok = |b: bool| if b { "✓" } else { "✗" };
    println!("tasks doctor");
    println!("  {} state DB: {}", ok(diag.db_present), diag.db_path.display());
    if diag.db_present {
        println!(
            "  {} state DB schema",
            ok(diag.db_schema_ok),
        );
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
    i32::from(!diag.is_ok())
}
