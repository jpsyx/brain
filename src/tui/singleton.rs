//! The one-interactive-brain-shell guard.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

#[must_use]
pub fn lock_path() -> PathBuf {
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".cache/brain/tui.lock"),
        |home| PathBuf::from(home).join(".cache/brain/tui.lock"),
    )
}

#[must_use]
pub fn lock_is_reclaimable(existing_pid: Option<i32>, pid_alive: bool) -> bool {
    existing_pid.is_none() || !pid_alive
}

pub struct Guard {
    file: File,
    path: PathBuf,
}

impl Guard {
    pub fn acquire() -> Result<Self> {
        let path = lock_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id()).context("writing brain singleton lock")?;
                Ok(Self { file, path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let pid = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|raw| raw.trim().parse().ok());
                if lock_is_reclaimable(pid, pid.is_some_and(crate::state::system_pid_alive)) {
                    let _ = std::fs::remove_file(&path);
                    return Self::acquire();
                }
                bail!("brain is already running (lock: {})", path.display());
            }
            Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.file.flush();
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pid_is_reclaimable() {
        assert!(lock_is_reclaimable(None, false));
    }

    #[test]
    fn live_pid_is_not_reclaimable() {
        assert!(!lock_is_reclaimable(Some(42), true));
    }

    #[test]
    fn dead_pid_is_reclaimable() {
        assert!(lock_is_reclaimable(Some(42), false));
    }
}
