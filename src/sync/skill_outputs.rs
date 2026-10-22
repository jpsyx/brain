//! Keep generated skills out of cloud sync and its existing baseline listings.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, ensure};
use regex::Regex;

pub(crate) const DIRECTORIES: [&str; 4] = [
    ".agents/skills",
    ".claude/skills",
    ".codex/skills",
    ".opencode/skills",
];

pub(crate) fn is_skill_output(path: &Path) -> bool {
    DIRECTORIES
        .iter()
        .any(|directory| path.starts_with(directory))
}

fn filtered_listing(contents: &str) -> Result<String> {
    static RECORD: OnceLock<Regex> = OnceLock::new();
    ensure!(
        contents.starts_with("# bisync listing v1 from "),
        "unsupported cached rclone listing format; skill-output filtering was not applied"
    );
    let record = RECORD.get_or_init(|| {
        Regex::new(
            r#"^[-d] +(-?\d+) \S+ \S+ \d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{9}[+-]\d{4} (\".+\")$"#,
        )
        .expect("valid rclone v1 listing record")
    });
    let mut filtered = String::with_capacity(contents.len());
    for (index, line) in contents.split_inclusive('\n').enumerate() {
        if index == 0 {
            filtered.push_str(line);
            continue;
        }
        let captures = record.captures(line.trim_end_matches('\n')).context(
            "unsupported cached rclone listing record; skill-output filtering was not applied",
        )?;
        let quoted = &captures[2];
        // Go's quoted paths leave these ASCII directory names and separators
        // literal. Matching components needs no decoding of unrelated names.
        let path = &quoted[1..quoted.len() - 1];
        if !is_skill_output(Path::new(path)) {
            filtered.push_str(line);
        }
    }
    Ok(filtered)
}

fn is_listing(name: &str) -> bool {
    [".lst", ".lst-old"].iter().any(|suffix| {
        name.strip_suffix(suffix).is_some_and(|prefix| {
            prefix
                .rsplit_once('.')
                .is_some_and(|(_, side)| matches!(side, "path1" | "path2"))
        })
    })
}

struct Update {
    path: PathBuf,
    original: String,
    filtered: String,
}

/// Remove only generated-skill records, retaining every other baseline byte.
///
/// The caller must hold the workspace sync lock. Original active and recovery
/// listings are durably backed up before any replacement. Retrying after a
/// partial replacement only reconciles the remaining files; no resync is needed.
pub fn reconcile_baselines(workdir: &Path) -> Result<usize> {
    let updates: Vec<_> = read_listings(workdir)?
        .into_iter()
        .filter(|update| update.filtered != update.original)
        .collect();
    if updates.is_empty() {
        return Ok(0);
    }
    let backup_root = workdir.join("skill-output-backups");
    match fs::symlink_metadata(&backup_root) {
        Ok(metadata) => ensure!(
            metadata.file_type().is_dir(),
            "sync listing backup path is not a directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_directory(&backup_root)?;
        }
        Err(error) => return Err(error.into()),
    }
    let backup = backup_root.join(uuid::Uuid::new_v4().to_string());
    create_private_directory(&backup)?;
    for update in &updates {
        write_new(
            &backup.join(update.path.file_name().context("listing filename")?),
            update.original.as_bytes(),
        )?;
    }
    File::open(&backup)?.sync_all()?;
    File::open(&backup_root)?.sync_all()?;
    File::open(workdir)?.sync_all()?;
    for update in &updates {
        let staged = workdir.join(format!(".brain-skill-filter-{}.tmp", uuid::Uuid::new_v4()));
        write_new(&staged, update.filtered.as_bytes())?;
        fs::rename(&staged, &update.path)?;
        File::open(workdir)?.sync_all()?;
    }
    Ok(updates.len())
}

fn read_listings(workdir: &Path) -> Result<Vec<Update>> {
    let entries = match fs::read_dir(workdir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut updates = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !is_listing(&entry.file_name().to_string_lossy()) {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file(),
            "cached rclone listing is not a regular file"
        );
        let original = fs::read_to_string(entry.path())?;
        let filtered = filtered_listing(&original)?;
        updates.push(Update {
            path: entry.path(),
            original,
            filtered,
        });
    }
    Ok(updates)
}

/// A disposable view for read-only sync previews.
pub struct PreviewBaselines {
    path: PathBuf,
}

impl PreviewBaselines {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PreviewBaselines {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Filter a private copy so previewing never consumes the live transition.
pub fn preview_baselines(workdir: &Path) -> Result<PreviewBaselines> {
    let listings = read_listings(workdir)?;
    let path = std::env::temp_dir().join(format!("brain-sync-preview-{}", uuid::Uuid::new_v4()));
    create_private_directory(&path)?;
    let preview = PreviewBaselines { path };
    for listing in listings {
        write_new(
            &preview
                .path
                .join(listing.path.file_name().context("listing filename")?),
            listing.filtered.as_bytes(),
        )?;
    }
    Ok(preview)
}

fn create_private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
#[path = "skill_outputs/tests.rs"]
mod tests;
