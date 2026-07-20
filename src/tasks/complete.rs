//! `tasks complete <id>` — mark a task done in `~/brain/tasks/{tasks,habits}.csv`.
//!
//! Hands off directly to `mark_done.py` in the `/todo` skill. That script
//! handles the CSV mutation (set status, completed_date, last_touched, habit
//! recurrence spawn, chunked-task `mit` migration) AND triggers the agenda
//! auto-update side effect (drop the task from `/tmp/<today>.md`, re-derive
//! habit sections, regen PDF when one exists) via
//! `update_agenda_on_mutation.py`. No Claude session is launched — marking a
//! task done is fully structured work, no NLP/judgement required.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, anyhow, bail};

/// Normalize a user-supplied ID into the canonical `T###` / `H###` form.
///
/// Accepts: `t123`, `T123`, `123` (assumed task), `h43`, `H43`. Any other
/// shape returns an error explaining the accepted forms.
pub fn normalize_id(raw: &str) -> Result<String> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("ID is required (try t123, T123, 123, or h43)");
    }
    let lower = s.to_ascii_lowercase();
    let (prefix, digits) = match lower.as_bytes().first() {
        Some(b't') => ('T', &lower[1..]),
        Some(b'h') => ('H', &lower[1..]),
        _ => ('T', lower.as_str()),
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("'{raw}' is not a valid ID (try t123, T123, 123, or h43)");
    }
    // Parse + reformat to drop any leading zeros (T0123 → T123) but keep the
    // exact value the user meant.
    let n: u32 = digits
        .parse()
        .map_err(|e| anyhow!("invalid number in ID '{raw}': {e}"))?;
    Ok(format!("{prefix}{n}"))
}

/// Replace the current process with `mark_done.py`. We `exec` rather than
/// spawn-and-wait so the script's stdout/stderr stream straight to the
/// user's terminal and signal handling stays clean.
pub fn run(raw_id: &str) -> Result<()> {
    let id = normalize_id(raw_id)?;

    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("$HOME is not set"))?;
    let home = PathBuf::from(home);

    let brain = home.join("brain");
    if !brain.is_dir() {
        bail!("{} does not exist", brain.display());
    }

    let mark_done = home
        .join("global-skills")
        .join("todo")
        .join("scripts")
        .join("mark_done.py");
    if !mark_done.is_file() {
        bail!(
            "mark_done.py not found at {} — install the /todo skill first",
            mark_done.display()
        );
    }

    let err = Command::new(&mark_done).arg(&id).exec();
    Err(anyhow!("failed to exec {}: {err}", mark_done.display()))
}

#[cfg(test)]
mod tests {
    use super::normalize_id;

    #[test]
    fn bare_number_assumes_task_prefix() {
        assert_eq!(normalize_id("123").unwrap(), "T123");
    }

    #[test]
    fn lowercase_t_becomes_uppercase() {
        assert_eq!(normalize_id("t42").unwrap(), "T42");
    }

    #[test]
    fn lowercase_h_becomes_uppercase() {
        assert_eq!(normalize_id("h7").unwrap(), "H7");
    }

    #[test]
    fn leading_zeros_are_stripped() {
        assert_eq!(normalize_id("T00123").unwrap(), "T123");
        assert_eq!(normalize_id("h007").unwrap(), "H7");
    }

    #[test]
    fn empty_input_errors() {
        assert!(normalize_id("").is_err());
        assert!(normalize_id("   ").is_err());
    }

    #[test]
    fn non_digit_after_prefix_errors() {
        assert!(normalize_id("Tfoo").is_err());
        assert!(normalize_id("h-1").is_err());
    }
}
