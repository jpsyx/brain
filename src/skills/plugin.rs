//! User plugins: whole skills the user owns, installed alongside the cores.
//!
//! They live in `<root>/.config/plugins/<name>/` (synced with the brain, never
//! committed to the public repo) and are installed by the same pipeline.

use std::fs;
use std::path::{Path, PathBuf};

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
        assert!(mine.files.iter().any(|f| f.rel_path == Path::new("SKILL.md")));
        assert!(mine.files.iter().any(|f| f.rel_path == Path::new("scripts/go.py")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_dir_yields_no_plugins() {
        assert!(discover(Path::new("/no/such/plugins/dir")).is_empty());
    }
}
