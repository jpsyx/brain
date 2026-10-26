//! Coordinate ordinary process lifetimes with installer finalization.
mod quiescence;
use anyhow::{Context as _, Result, ensure};
use fs2::FileExt as _;
use std::{
    fs::{self, File},
    io::Read as _,
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Shared ownership remains alive until ordinary dispatch, including a TUI, exits.
pub struct Guard {
    _file: File,
}
pub(super) struct ExclusiveGuard {
    _guard: Guard,
}

impl Guard {
    /// Refuse installation contention and stale executables before ordinary work.
    pub fn ordinary() -> Result<Self> {
        Self::ordinary_at(&super::home_dir()?, &std::env::current_exe()?)
    }

    fn ordinary_at(home: &Path, executable: &Path) -> Result<Self> {
        let guard = Self::acquire(home, false)?;
        verify_executable(executable, env!("CARGO_PKG_VERSION"))
            .context("Brain executable changed; restart the command and retry")?;
        Ok(guard)
    }

    pub(super) fn exclusive() -> Result<ExclusiveGuard> {
        let home = super::home_dir()?;
        let guard = Self::acquire(&home, true)?;
        quiescence::check(&home).context(
            "Close Brain TUIs and wait for active commands to finish, then retry installation",
        )?;
        Ok(ExclusiveGuard { _guard: guard })
    }

    #[cfg(test)]
    pub(super) fn exclusive_for_test(home: &Path) -> Result<ExclusiveGuard> {
        Ok(ExclusiveGuard {
            _guard: Self::acquire(home, true)?,
        })
    }

    fn acquire(home: &Path, exclusive: bool) -> Result<Self> {
        let directory = home.join(".cache/brain");
        crate::skills::install_transaction::prepare_cache_directory(&directory)?;
        let path = directory.join("invocations.lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)?;
        let held = file.metadata()?;
        ensure!(
            held.is_file() && held.uid() == fs::metadata(home)?.uid(),
            "unsafe Brain invocation lock"
        );
        if exclusive { file.try_lock_exclusive() } else { fs2::FileExt::try_lock_shared(&file) }
            .context("Brain is active or installation is in progress; close Brain TUIs, wait for commands to finish, then retry")?;
        let entry = fs::symlink_metadata(path)?;
        ensure!(
            entry.is_file() && entry.dev() == held.dev() && entry.ino() == held.ino(),
            "Brain invocation lock changed; retry"
        );
        Ok(Self { _file: file })
    }
}

pub(super) fn verify_executable(path: &Path, version: &str) -> Result<()> {
    let before = fs::symlink_metadata(path)?;
    ensure!(before.is_file(), "Brain executable is not a regular file");
    let mut child = Command::new(path)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Brain executable version probe timed out");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut output = String::new();
    child
        .stdout
        .take()
        .context("missing version output")?
        .take(128)
        .read_to_string(&mut output)?;
    let after = fs::symlink_metadata(path)?;
    ensure!(
        status.success()
            && output.trim() == format!("brain {version}")
            && before.dev() == after.dev()
            && before.ino() == after.ino(),
        "Brain executable changed; restart the command and retry"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn process_loaded_before_replacement_refuses_the_new_disk_version() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("brain");
        std::fs::write(&executable, "#!/bin/sh\necho 'brain 0.101.0'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ownership = Guard::exclusive_for_test(root.path()).unwrap();
        let busy = Guard::ordinary_at(root.path(), &executable).err().unwrap();
        assert!(busy.to_string().contains("Brain is active"));
        drop(ownership);
        let stale = Guard::ordinary_at(root.path(), &executable).err().unwrap();
        assert!(stale.to_string().contains("executable changed"));
    }

    #[test]
    fn unlinked_running_executable_refuses_ordinary_work() {
        let root = tempfile::tempdir().unwrap();
        assert!(verify_executable(&root.path().join("gone"), env!("CARGO_PKG_VERSION")).is_err());
    }
}
