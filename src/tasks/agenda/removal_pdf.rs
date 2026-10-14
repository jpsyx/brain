//! Stage removal-only printables before replacing the previous copy.
use super::Targets;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

/// Only an exit-successful, nonempty regular output can replace the old PDF.
/// This checks production/existence, not PDF syntax. All staging lives on the
/// destination filesystem so the final rename is an atomic replacement.
pub(super) fn replace_printable(targets: &Targets, markdown: &str) -> std::io::Result<()> {
    let renderer = targets
        .renderer
        .as_ref()
        .ok_or_else(|| std::io::Error::other("renderer unavailable"))?;
    let parent = targets
        .pdf
        .parent()
        .ok_or_else(|| std::io::Error::other("printable parent unavailable"))?;
    let directory = parent.join(format!(".brain-agenda-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory)?;
    let staged = Staged {
        source: directory.join("source.md"),
        output: directory.join("agenda.pdf"),
        directory,
    };
    std::fs::write(&staged.source, super::io::strip_html_comments(markdown))?;
    let status = Command::new(renderer)
        .arg(&staged.source)
        .arg("--out")
        .arg(&staged.output)
        .arg("--agenda")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other("renderer failed"));
    }
    let metadata = std::fs::symlink_metadata(&staged.output)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(std::io::Error::other("renderer output unavailable"));
    }
    std::fs::rename(&staged.output, &targets.pdf)
}

struct Staged {
    directory: PathBuf,
    source: PathBuf,
    output: PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        // The unique directory belongs solely to this attempt, including invalid
        // renderer artifacts. remove_dir_all does not follow output symlinks.
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
