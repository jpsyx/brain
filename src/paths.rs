//! Legacy single-workspace root resolution.
//!
//! Ordinary commands select a schema-v2 [`crate::workspace::WorkspaceContext`]
//! before accessing workspace-owned state and use its immutable root snapshot.
//! These helpers remain for legacy migration compatibility: a pre-migration
//! flat `root`, else the read-only `~/.config/brain-root` pointer, else
//! `$HOME/brain`.
//!
//! In schema v2, `root` is a validated structural registry field rather than a
//! writable free-form env variable. The legacy pointer is never written.
//!
//! The IO-free pieces (`resolve_root`, `parse_root_key`, `parse_brain_root_file`,
//! `expand_tilde_with_home`) are split out from the env/filesystem-touching
//! wrappers so they can be unit tested without a real `$HOME`, a real
//! `env.json`, or a real `brain-root` file.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Resolve and create the legacy/default root for migration compatibility.
pub fn brain_root() -> Result<PathBuf> {
    let root = brain_root_path();
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

/// Resolve the legacy/default root path without requiring it to exist.
#[must_use]
pub fn brain_root_path() -> PathBuf {
    let home = home_dir().unwrap_or_default();
    resolve_root(
        read_env_root().as_deref(),
        read_brain_root_file().as_deref(),
        &home,
    )
}

/// Pure legacy-root precedence: the flat/schema-default `root`, else the legacy
/// `~/.config/brain-root` pointer, else the `<home>/brain` default. Each
/// candidate is tilde-expanded against `home`.
#[must_use]
pub fn resolve_root(env_key: Option<&str>, legacy_pointer: Option<&str>, home: &Path) -> PathBuf {
    let pick = env_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| legacy_pointer.map(str::trim).filter(|s| !s.is_empty()));
    pick.map_or_else(
        || home.join("brain"),
        |raw| expand_tilde_with_home(raw, home),
    )
}

/// Read the `root` field from `~/.config/brain/env.json`, if any. A missing
/// file/field reads as `None` so resolution falls through to the legacy pointer.
fn read_env_root() -> Option<String> {
    std::fs::read_to_string(machine_config_dir().join("env.json"))
        .ok()
        .as_deref()
        .and_then(parse_root_key)
}

/// Pull the default workspace root, or a pre-migration flat `root`, out of an
/// `env.json` body. Pure: no IO. Missing, blank, or invalid input is `None`.
#[must_use]
pub fn parse_root_key(env_json: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(env_json).ok()?;
    value
        .get("root")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            let default = value.get("default_workspace")?.as_str()?;
            value.get("workspaces")?.get(default)?.get("root")?.as_str()
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The machine-local brain-root pointer file: `$XDG_CONFIG_HOME/brain-root` or
/// `~/.config/brain-root`. This is a plain `$HOME`-side path (NOT inside the
/// brain root), so reading it never depends on the root it resolves.
fn brain_root_file() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map_or_else(
            || home_dir().unwrap_or_default().join(".config"),
            PathBuf::from,
        );
    config_home.join("brain-root")
}

/// The machine-local brain-env directory.
///
/// `$XDG_CONFIG_HOME/brain` or `~/.config/brain`. It holds the schema-v2
/// workspace registry in `env.json`. Unlike a workspace's internal config dir,
/// it lives at a fixed `$HOME`-side path and never rides workspace sync.
#[must_use]
pub fn machine_config_dir() -> PathBuf {
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty());
    let home = home_dir().unwrap_or_default();
    machine_config_dir_from(xdg.as_deref(), &home)
}

/// Pure core of [`machine_config_dir`]: `<xdg>/brain`, else `<home>/.config/brain`.
#[must_use]
pub fn machine_config_dir_from(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    let base = xdg_config_home
        .filter(|s| !s.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from);
    base.join("brain")
}

/// Read + parse the brain-root pointer. `None` when the file is absent, empty,
/// or unreadable (so the caller falls back to the default root).
fn read_brain_root_file() -> Option<String> {
    std::fs::read_to_string(brain_root_file())
        .ok()
        .as_deref()
        .and_then(parse_brain_root_file)
}

/// Parse the contents of a `brain-root` file into a path string. Pure: no IO.
/// The file holds a single path; surrounding whitespace/newlines are trimmed and
/// an empty file is treated as "unset" (`None`).
#[must_use]
pub fn parse_brain_root_file(contents: &str) -> Option<String> {
    let trimmed = contents.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Pure tilde expansion against an explicit home directory.
#[must_use]
pub fn expand_tilde_with_home(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

fn home_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("$HOME is not set"))?;
    Ok(PathBuf::from(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_the_path_line() {
        assert_eq!(parse_brain_root_file("~/brain"), Some("~/brain".to_owned()));
        assert_eq!(
            parse_brain_root_file("/srv/brain"),
            Some("/srv/brain".to_owned())
        );
    }

    #[test]
    fn parse_trims_surrounding_whitespace_and_newlines() {
        assert_eq!(
            parse_brain_root_file("  ~/brain \n"),
            Some("~/brain".to_owned())
        );
    }

    #[test]
    fn empty_or_blank_file_is_unset() {
        assert_eq!(parse_brain_root_file(""), None);
        assert_eq!(parse_brain_root_file("   \n\t"), None);
    }

    #[test]
    fn tilde_slash_expands_against_home() {
        let home = Path::new("/Users/x");
        assert_eq!(
            expand_tilde_with_home("~/brain/projects", home),
            PathBuf::from("/Users/x/brain/projects")
        );
    }

    #[test]
    fn bare_tilde_is_home() {
        assert_eq!(
            expand_tilde_with_home("~", Path::new("/Users/x")),
            PathBuf::from("/Users/x")
        );
    }

    #[test]
    fn absolute_path_passes_through_untouched() {
        assert_eq!(
            expand_tilde_with_home("/etc/brain", Path::new("/Users/x")),
            PathBuf::from("/etc/brain")
        );
    }

    #[test]
    fn mid_string_tilde_is_not_expanded() {
        // Only a *leading* ~ is a home reference.
        assert_eq!(
            expand_tilde_with_home("/a/~/b", Path::new("/Users/x")),
            PathBuf::from("/a/~/b")
        );
    }

    #[test]
    fn machine_config_dir_prefers_xdg_config_home() {
        assert_eq!(
            machine_config_dir_from(Some("/xdg"), Path::new("/Users/x")),
            PathBuf::from("/xdg/brain")
        );
    }

    #[test]
    fn machine_config_dir_falls_back_to_home_dotconfig() {
        assert_eq!(
            machine_config_dir_from(None, Path::new("/Users/x")),
            PathBuf::from("/Users/x/.config/brain")
        );
    }

    #[test]
    fn resolve_root_prefers_the_env_key_over_the_legacy_pointer() {
        let home = Path::new("/Users/x");
        assert_eq!(
            resolve_root(Some("~/work-brain"), Some("~/old"), home),
            PathBuf::from("/Users/x/work-brain")
        );
    }

    #[test]
    fn resolve_root_falls_back_to_the_legacy_pointer_then_default() {
        let home = Path::new("/Users/x");
        assert_eq!(
            resolve_root(None, Some("/srv/brain"), home),
            PathBuf::from("/srv/brain")
        );
        assert_eq!(
            resolve_root(None, None, home),
            PathBuf::from("/Users/x/brain")
        );
    }

    #[test]
    fn parse_root_key_reads_the_string_field() {
        assert_eq!(
            parse_root_key(r#"{"root": "~/brain"}"#),
            Some("~/brain".to_owned())
        );
        assert_eq!(parse_root_key(r#"{"root": ""}"#), None);
        assert_eq!(parse_root_key(r#"{"markdown_to_pdf_path": "x"}"#), None);
        assert_eq!(parse_root_key("not json"), None);
    }

    #[test]
    fn parse_root_key_reads_the_default_registry_workspace() {
        // Version-agnostic on purpose: this is the legacy pointer fallback, and
        // it must keep reading a root out of whatever schema it is handed.
        let registry = r#"{
            "schema_version": 3,
            "default_workspace": "family",
            "workspaces": {
                "brain": {
                    "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
                    "root": "/workspaces/brain",
                    "aliases": [],
                    "local_user_id": ""
                },
                "family": {
                    "workspace_id": "e806258e-491a-436d-9db4-a5ca9903e0d4",
                    "root": "/workspaces/family",
                    "aliases": [],
                    "local_user_id": ""
                }
            }
        }"#;

        assert_eq!(
            parse_root_key(registry),
            Some("/workspaces/family".to_owned())
        );
    }
}
