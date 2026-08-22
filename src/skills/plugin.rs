//! User plugins: whole skills the user owns, installed alongside the cores.
//!
//! They live in `<root>/.config/plugins/<name>/` (synced with the brain, never
//! committed to the public repo) and are installed by the same pipeline.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::model::{Skill, SkillFile};

/// Discover plugins under `dir` (each immediate subdirectory is one plugin).
/// A missing/unreadable dir yields no plugins.
#[must_use]
pub fn discover(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut plugins = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let mut files = Vec::new();
        collect(&path, &path, &mut files);
        if !files.is_empty() {
            plugins.push(Skill { name, files });
        }
    }
    plugins
}

/// Discover user-authored skills already living in a workspace skill directory.
///
/// Only real directories with a regular `SKILL.md` are eligible; malformed
/// entries are left untouched for the user to repair.
#[must_use]
pub fn discover_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return None;
            }
            let skill = path.join("SKILL.md");
            let skill_metadata = fs::symlink_metadata(skill).ok()?;
            if skill_metadata.is_file() && !skill_metadata.file_type().is_symlink() {
                entry.file_name().into_string().ok()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Validate one exact machine skill directory and return its stable canonical path.
pub(crate) fn validate_exact_path(path: &Path) -> Result<PathBuf> {
    let canonical = canonical_path_below_trusted_root(path)?;
    load_exact_from("validation", &canonical)?;
    Ok(canonical)
}

/// Load only the configured machine skill directory without inspecting siblings.
pub(crate) fn load_exact(name: &str, path: &Path) -> Result<Skill> {
    let canonical = canonical_path_below_trusted_root(path)?;
    load_exact_from(name, &canonical)
}

fn load_exact_from(name: &str, path: &Path) -> Result<Skill> {
    validate_path_components(path)?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("reading machine skill directory {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("machine skill path cannot be a symlink");
    }
    if !metadata.is_dir() {
        bail!("machine skill path is not a directory");
    }
    let skill_file = path.join("SKILL.md");
    let skill_metadata = fs::symlink_metadata(&skill_file)
        .with_context(|| format!("reading {}", skill_file.display()))?;
    if skill_metadata.file_type().is_symlink() || !skill_metadata.is_file() {
        bail!("machine skill SKILL.md must be a regular file, not a symlink");
    }
    let mut files = Vec::new();
    collect_exact(path, path, &mut files)?;
    Ok(Skill {
        name: name.to_owned(),
        files,
    })
}

fn canonical_path_below_trusted_root(path: &Path) -> Result<PathBuf> {
    validate_path_components(path)?;
    // The root-owned first component is the trust anchor. This permits
    // platform aliases such as `/var` while every caller-controlled component
    // below it must be a real directory. The canonical containment check keeps
    // the anchor alias from resolving into an unrelated top-level tree.
    let trusted_root = trusted_top_level(path)?;
    let canonical_root = fs::canonicalize(&trusted_root).with_context(|| {
        format!(
            "canonicalizing trusted machine skill root {}",
            trusted_root.display()
        )
    })?;
    validate_ancestors_below(path, &trusted_root)?;
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("canonicalizing machine skill directory {}", path.display()))?;
    if !canonical.starts_with(&canonical_root) {
        bail!(
            "machine skill path resolves outside trusted root {}",
            canonical_root.display()
        );
    }
    validate_ancestors_below(&canonical, &trusted_top_level(&canonical)?)?;
    Ok(canonical)
}

fn trusted_top_level(path: &Path) -> Result<PathBuf> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::RootDir)) {
        bail!("machine skill path must be absolute");
    }
    let Some(Component::Normal(first)) = components.next() else {
        bail!("machine skill path must name a directory below a trusted top-level root");
    };
    Ok(Path::new("/").join(first))
}

fn validate_ancestors_below(path: &Path, trusted_root: &Path) -> Result<()> {
    let relative = path.strip_prefix(trusted_root).with_context(|| {
        format!(
            "machine skill path {} left its trusted root",
            path.display()
        )
    })?;
    let mut current = trusted_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!("machine skill path cannot contain traversal components");
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("reading machine skill ancestor {}", current.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("machine skill path ancestor cannot be a symlink");
        }
        if !metadata.is_dir() {
            bail!("machine skill path ancestor is not a directory");
        }
    }
    Ok(())
}

fn validate_path_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("machine skill path must be absolute");
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        bail!("machine skill path cannot contain traversal components");
    }
    Ok(())
}

fn collect_exact(root: &Path, dir: &Path, out: &mut Vec<SkillFile>) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("reading machine skill directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("reading machine skill entry {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("machine skill source cannot contain symlinks");
        }
        if metadata.is_dir() {
            collect_exact(root, &path, out)?;
        } else if metadata.is_file() {
            out.push(SkillFile {
                rel_path: path
                    .strip_prefix(root)
                    .expect("collected path stays below exact source")
                    .to_path_buf(),
                contents: fs::read(&path)
                    .with_context(|| format!("reading machine skill file {}", path.display()))?,
            });
        } else {
            bail!("machine skill source contains a non-file entry");
        }
    }
    Ok(())
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<SkillFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if let (Ok(rel), Ok(contents)) = (path.strip_prefix(root), fs::read(&path)) {
            out.push(SkillFile {
                rel_path: rel.to_path_buf(),
                contents,
            });
        }
    }
}

/// The plugins dir under the brain config dir: `<config-dir>/plugins`.
#[must_use]
pub fn dir_in_config(config_dir: &Path) -> PathBuf {
    config_dir.join("plugins")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static C: AtomicU32 = AtomicU32::new(0);

    fn tmp() -> PathBuf {
        let n = C.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("brain-plugin-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn discovers_a_plugin_with_its_files() {
        let root = tmp();
        let p = root.join("my-plugin");
        fs::create_dir_all(p.join("scripts")).unwrap();
        fs::write(p.join("SKILL.md"), "# mine").unwrap();
        fs::write(p.join("scripts").join("go.py"), "print(1)").unwrap();

        let found = discover(&root);
        let mine = found.iter().find(|s| s.name == "my-plugin").unwrap();
        assert_eq!(mine.files.len(), 2);
        assert!(
            mine.files
                .iter()
                .any(|f| f.rel_path == Path::new("SKILL.md"))
        );
        assert!(
            mine.files
                .iter()
                .any(|f| f.rel_path == Path::new("scripts/go.py"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_dir_yields_no_plugins() {
        assert!(discover(Path::new("/no/such/plugins/dir")).is_empty());
    }

    #[test]
    fn discovers_workspace_skill_names_without_following_symlinks() {
        let root = tmp();
        let skill = root.join("mine");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# mine").unwrap();
        let malformed = root.join("malformed");
        fs::create_dir_all(&malformed).unwrap();
        let target = root.join("target.md");
        fs::write(&target, "# target").unwrap();
        std::os::unix::fs::symlink(&target, malformed.join("SKILL.md")).unwrap();

        assert_eq!(discover_names(&root), vec!["mine"]);
        let _ = fs::remove_dir_all(&root);
    }
}
