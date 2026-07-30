//! Per-run logging, with optional stdout mirroring.

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::{Local, SecondsFormat};

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static STDOUT_ENABLED: AtomicBool = AtomicBool::new(false);

/// Owns this run's verbose logger lifecycle.
pub struct Guard {
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        log("brain end");
        if stdout_enabled() {
            println!("verbose log: {}", self.path.display());
        }
    }
}

/// File-backed verbose logger.
pub(crate) struct Logger {
    file: File,
}

impl Logger {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map(|file| Self { file })
    }

    pub(crate) fn write(&mut self, message: &str) -> io::Result<()> {
        writeln!(self.file, "{message}")?;
        self.file.flush()
    }
}

pub fn init(verbose: bool, stdout: bool) -> io::Result<Guard> {
    STDOUT_ENABLED.store(verbose && stdout, Ordering::Relaxed);
    let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Nanos, false);
    let path = log_path_for(&timestamp);
    let logger = Logger::open(&path)?;
    let _ = LOG_PATH.set(path.clone());
    let _ = LOGGER.set(Mutex::new(logger));
    log(format!("brain start {}", env!("CARGO_PKG_VERSION")));
    Ok(Guard { path })
}

pub fn log(message: impl AsRef<str>) {
    let message = message.as_ref();
    let line = format!(
        "{} {message}",
        Local::now().to_rfc3339_opts(SecondsFormat::Nanos, false)
    );
    if let Some(logger) = LOGGER.get() {
        let _ = logger.lock().map(|mut logger| logger.write(&line));
    }
    if stdout_enabled() {
        println!("{line}");
    }
}

pub fn path() -> Option<PathBuf> {
    LOG_PATH.get().cloned()
}

pub fn set_stdout_enabled(enabled: bool) {
    STDOUT_ENABLED.store(enabled, Ordering::Relaxed);
}

fn stdout_enabled() -> bool {
    STDOUT_ENABLED.load(Ordering::Relaxed)
}

#[must_use]
pub(crate) fn log_path_for(timestamp: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "{timestamp}-{}.log",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    #[test]
    fn log_path_uses_tmp_and_the_exact_timestamp_as_filename() {
        let path = log_path_for("2026-07-26T10:11:12.123456789-04:00");
        assert!(path
            .to_string_lossy()
            .starts_with("/tmp/2026-07-26T10:11:12.123456789-04:00-"));
        assert!(path.extension().is_some_and(|ext| ext == "log"));
    }

    #[test]
    fn logger_writes_lines_to_the_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut logger = Logger::open(file.path()).unwrap();

        logger.write("hello world").unwrap();

        let mut text = String::new();
        std::fs::File::open(file.path())
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.contains("hello world"), "{text}");
    }
}
