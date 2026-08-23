//! Automatic, version-directed machine migrations.

mod lifecycle;
mod receiver_model;
mod version;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use version::Version;

const LIFECYCLE_VERSION: Version = Version::new(0, 71, 0);
const RECEIVER_MODEL_VERSION: Version = Version::new(0, 72, 0);
const PRE_MIGRATION_VERSION: Version = Version::new(0, 70, 0);

struct Migration {
    introduced: Version,
    up: fn(&Path) -> Result<()>,
    down: fn(&Path) -> Result<()>,
}

const MIGRATIONS: [Migration; 2] = [
    Migration {
        introduced: LIFECYCLE_VERSION,
        up: lifecycle::up,
        down: lifecycle::down,
    },
    Migration {
        introduced: RECEIVER_MODEL_VERSION,
        up: receiver_model::up,
        down: receiver_model::down,
    },
];

/// Reconcile every migration owned by this binary before ordinary dispatch.
pub fn run_current() -> Result<()> {
    let home = home_dir()?;
    let state_path = state_path();
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let recorded = read_state(&state_path)?;
    let from = recorded.unwrap_or(PRE_MIGRATION_VERSION);
    run(&home, from, current, true)?;
    if recorded != Some(current) {
        // Reconciliation is idempotent, so an unwritable stamp only means the
        // next ordinary command will repeat it. Do not mask that command's own
        // diagnostics when its config path is intentionally read-only.
        let _ = write_state(&state_path, current);
    }
    Ok(())
}

/// Run an installer-requested upgrade or downgrade without ordinary dispatch.
pub fn run_explicit(from: &str, to: &str) -> Result<()> {
    let home = home_dir()?;
    let from = Version::parse(from).context("parse installed Brain version")?;
    let to = Version::parse(to).context("parse target Brain version")?;
    let binary = Version::parse(env!("CARGO_PKG_VERSION"))?;
    anyhow::ensure!(
        from == binary || to == binary,
        "migration must start or end at this binary's version {binary}"
    );
    run(&home, from, to, from == to)?;
    write_state(&state_path(), to)
}

fn run(home: &Path, from: Version, to: Version, reconcile: bool) -> Result<()> {
    if from < to {
        for migration in MIGRATIONS
            .iter()
            .filter(|migration| migration.introduced > from && migration.introduced <= to)
        {
            (migration.up)(home)?;
        }
    } else if from > to {
        for migration in MIGRATIONS
            .iter()
            .rev()
            .filter(|migration| migration.introduced <= from && migration.introduced > to)
        {
            (migration.down)(home)?;
        }
    } else if reconcile {
        for migration in MIGRATIONS
            .iter()
            .filter(|migration| migration.introduced <= to)
        {
            (migration.up)(home)?;
        }
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

fn state_path() -> PathBuf {
    crate::paths::machine_config_dir().join("migrations/version")
}

fn read_state(path: &Path) -> Result<Option<Version>> {
    match std::fs::read_to_string(path) {
        Ok(value) => Version::parse(value.trim()).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("read migration state {}", path.display()))
        }
    }
}

fn write_state(path: &Path, version: Version) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create migration state directory {}", parent.display()))?;
    let temporary = parent.join(format!(".version.tmp-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create migration state {}", temporary.display()))?;
    let result = (|| {
        writeln!(file, "{version}")
            .with_context(|| format!("write migration state {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync migration state {}", temporary.display()))?;
        drop(file);
        std::fs::rename(&temporary, path).with_context(|| {
            format!(
                "replace migration state {} from {}",
                path.display(),
                temporary.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
