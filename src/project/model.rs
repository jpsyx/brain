//! A project's `.METADATA.json` — the canonical record the lookup CSV is
//! rebuilt from.
//!
//! The file is the source of truth, so writing it is worth doing exactly:
//! the field set is fixed, `name` must match the folder, `namespace` must match
//! the prefix of `name`, and `directory` must match where the folder actually
//! is. Every one of those is mechanical, and every one of them has been got
//! wrong by hand.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Where a project can be in its life.
pub(crate) const STATUSES: [&str; 5] = [
    "not-started",
    "in-progress",
    "blocked",
    "extracting-ips",
    "done",
];

/// The same scale task priorities use.
pub(crate) const PRIORITIES: [&str; 5] = ["p0", "p1", "p2", "p3", "p4"];

/// `due` when there is genuinely no deadline.
pub(crate) const NO_DUE: &str = "none";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Metadata {
    pub(crate) name: String,
    pub(crate) namespace: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) priority: String,
    pub(crate) due: String,
    pub(crate) directory: String,
    #[serde(default)]
    pub(crate) tasks: Vec<String>,
    /// Anything a caller added that core does not model is carried through
    /// untouched rather than dropped on the next write.
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

/// The namespace half of a `<namespace>__<outcome>` slug.
pub(crate) fn namespace_of(slug: &str) -> Option<&str> {
    slug.split_once("__")
        .map(|(namespace, _)| namespace)
        .filter(|namespace| !namespace.is_empty())
}

/// Validate a project slug: `<namespace>__<outcome>`, lowercase kebab.
pub(crate) fn validate_slug(slug: &str) -> Result<String> {
    let slug = slug.trim();
    let Some(namespace) = namespace_of(slug) else {
        bail!("'{slug}' is not a project slug (expected <namespace>__<outcome>)");
    };
    let outcome = &slug[namespace.len() + 2..];
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    if !segment_ok(namespace)
        || !outcome.split('-').all(|part| !part.is_empty())
        || !segment_ok(outcome)
    {
        bail!(
            "'{slug}' is not a project slug: use lowercase letters, digits, and '-', \
             joined as <namespace>__<outcome>"
        );
    }
    Ok(slug.to_owned())
}

pub(crate) fn validate_status(status: &str) -> Result<String> {
    let status = status.trim();
    if STATUSES.contains(&status) {
        Ok(status.to_owned())
    } else {
        bail!("status must be one of {}", STATUSES.join(", "))
    }
}

pub(crate) fn validate_priority(priority: &str) -> Result<String> {
    let priority = priority.trim();
    if PRIORITIES.contains(&priority) {
        Ok(priority.to_owned())
    } else {
        bail!("priority must be one of {}", PRIORITIES.join(", "))
    }
}

/// Validate a due date: an absolute `YYYY-MM-DD`, or `none`.
///
/// Deliberately strict. "next month" on a dashboard field is a date nobody can
/// sort by, and a project's due date is exactly what gets sorted by.
pub(crate) fn validate_due(due: &str) -> Result<String> {
    let due = due.trim();
    if due.eq_ignore_ascii_case(NO_DUE) || due.is_empty() {
        return Ok(NO_DUE.to_owned());
    }
    if chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d").is_ok() {
        Ok(due.to_owned())
    } else {
        bail!("due must be an absolute YYYY-MM-DD date, or '{NO_DUE}'")
    }
}

impl Metadata {
    /// A fresh project's record.
    pub(crate) fn new(slug: &str, title: &str, status: &str, priority: &str, due: &str) -> Self {
        Self {
            namespace: namespace_of(slug).unwrap_or_default().to_owned(),
            directory: format!("projects/{slug}"),
            name: slug.to_owned(),
            title: title.to_owned(),
            status: status.to_owned(),
            priority: priority.to_owned(),
            due: due.to_owned(),
            tasks: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    /// Bring `name`, `namespace`, and `directory` back in line with where the
    /// project actually lives.
    pub(crate) fn realign(&mut self, slug: &str, directory: &str) {
        slug.clone_into(&mut self.name);
        namespace_of(slug)
            .unwrap_or_default()
            .clone_into(&mut self.namespace);
        directory.clone_into(&mut self.directory);
    }
}

pub(crate) fn metadata_path(directory: &Path) -> PathBuf {
    directory.join(".METADATA.json")
}

pub(crate) fn load(directory: &Path) -> Result<Metadata> {
    let path = metadata_path(directory);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| anyhow::anyhow!("read {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| anyhow::anyhow!("parse {}: {error}", path.display()))
}

pub(crate) fn save(directory: &Path, metadata: &Metadata) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    std::fs::write(
        metadata_path(directory),
        serde_json::to_string_pretty(metadata)? + "\n",
    )?;
    Ok(())
}

/// The README a new project starts with: an H1 and the outcome, nothing else.
/// Status and dates live in the metadata, so a second copy here would only rot.
pub(crate) fn readme(title: &str, description: &str) -> String {
    let description = description.trim();
    if description.is_empty() {
        format!("# {title}\n")
    } else {
        format!("# {title}\n\n{description}\n")
    }
}
