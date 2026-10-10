use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::{ZipWriter, write::SimpleFileOptions};

/// Recursively omit recognized application state and regenerable tool artifacts.
fn excluded(name: &str, is_dir: bool) -> bool {
    crate::clean::is_byproduct(name, is_dir)
        || matches!(
            name,
            ".env"
                | ".venv"
                | ".METADATA.json"
                | "AGENTS.md"
                | "CLAUDE.md"
                | ".config"
                | ".cache"
                | ".agents"
                | ".claude"
                | ".codex"
                | ".opencode"
                | ".pi"
                | ".windsurf"
                | ".git"
                | ".hg"
                | ".svn"
                | "node_modules"
                | "__MACOSX"
                | ".Spotlight-V100"
                | ".Trashes"
                | ".fseventsd"
                | "desktop.ini"
                | "Desktop.ini"
                | "ehthumbs.db"
                | ".sync-check"
                | "RCLONE_TEST"
                | "projects-lookup.csv"
                | "zotero-lookup.csv"
                | ".tasks_next_id"
                | ".habits_next_id"
        )
        || name.starts_with(".env.")
        || name.starts_with("._")
        || name.starts_with(".brain-")
        || name.contains(".brain-triage-")
        || name.ends_with(".transaction.lock")
        || Path::new(name).extension().is_some_and(|extension| {
            ["pyc", "pyo", "swp", "swo"]
                .iter()
                .any(|suffix| extension.eq_ignore_ascii_case(suffix))
        })
        || name.ends_with('~')
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Build `<directory>/<directory-name>.zip`, atomically replacing a prior ZIP.
/// Symlinks and special files are omitted; the existing output is never an input.
pub fn create_archive(directory: &Path) -> Result<PathBuf> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("The directory needs a UTF-8 name"))?;
    let destination = directory.join(format!("{name}.zip"));
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if !metadata.is_file() {
            bail!(
                "ZIP destination is not a regular file: {}",
                destination.display()
            );
        }
    }
    let pending = Pending(directory.join(format!(".brain-share-{}.tmp", uuid::Uuid::new_v4())));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending.0)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
        .large_file(true);
    let walk = WalkDir::new(directory)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || (!excluded(
                    &entry.file_name().to_string_lossy(),
                    entry.file_type().is_dir(),
                ) && entry.path() != destination)
        });
    for entry in walk {
        let entry = entry.context("Read directory for sharing")?;
        if entry.depth() == 0 {
            continue;
        }
        let kind = entry.file_type();
        if !kind.is_file() && !kind.is_dir() {
            continue;
        }
        let relative = entry.path().strip_prefix(directory)?;
        let name = relative.to_str().context("ZIP member name is not UTF-8")?;
        if kind.is_dir() {
            zip.add_directory(format!("{name}/"), options.unix_permissions(0o755))?;
        } else {
            // A no-follow open also protects a file replaced with a symlink after walking.
            let mut open = OpenOptions::new();
            open.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                open.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
            }
            let mut input = open
                .open(entry.path())
                .with_context(|| format!("Read {} for ZIP", relative.display()))?;
            if !input.metadata()?.is_file() {
                bail!("ZIP input changed: {}", relative.display());
            }
            zip.start_file(name, options)?;
            std::io::copy(&mut input, &mut zip)?;
        }
    }
    let file = zip.finish()?;
    file.sync_all()?;
    drop(file);
    fs::rename(&pending.0, &destination).context("Publish completed ZIP")?;
    File::open(directory)?.sync_all()?;
    Ok(destination)
}
