//! Applying a render to disk.
//!
//! Collects the bundled skills plus the user's plugins, injects each skill's
//! extension, writes the built copies, and creates the registry + frontend
//! symlinks. Link *targets* come from the pure `layout::link_ops`; this module
//! is the thin FS shell.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::layout::{Layout, Link, WorkspaceCapabilityLayout, link_ops};
use super::model::Skill;
use super::{embed, extension, plugin, render};

/// Where the user's extensions and plugins are read from. Both optional; a
/// public user with neither just gets the bundled skills verbatim.
#[derive(Default)]
pub struct Sources {
    pub extensions_dir: Option<PathBuf>,
    pub plugins_dir: Option<PathBuf>,
}

/// What a sync did.
pub struct Report {
    pub installed: Vec<String>,
}

/// What one cache-local selected skill render produced.
pub struct WorkspaceCapabilityReport {
    pub rendered_dir: PathBuf,
    pub rendered: Vec<String>,
}

/// Render + install every bundled skill and plugin into `layout`, injecting
/// extensions from `sources`. Idempotent.
pub fn sync(layout: &Layout, sources: &Sources) -> Result<Report> {
    let mut skills = embed::bundled_skills();
    crate::logging::log(format!("skills bundled count={}", skills.len()));
    if let Some(dir) = &sources.plugins_dir {
        let plugins = plugin::discover(dir);
        crate::logging::log(format!(
            "skills plugin dir={} count={}",
            dir.display(),
            plugins.len()
        ));
        skills.extend(plugins);
    }

    let mut installed = Vec::new();
    for skill in &skills {
        crate::logging::log(format!("skills install {}", skill.name));
        let ext = sources
            .extensions_dir
            .as_deref()
            .and_then(|d| extension::load(&skill.name, d));
        crate::logging::log(format!(
            "skills extension {} loaded={}",
            skill.name,
            ext.is_some()
        ));
        write_built(skill, ext.as_ref(), layout)?;
        for link in link_ops(&skill.name, layout) {
            create_symlink(&link)?;
        }
        installed.push(skill.name.clone());
    }
    Ok(Report { installed })
}

fn write_built(skill: &Skill, ext: Option<&extension::Extension>, layout: &Layout) -> Result<()> {
    write_built_to(skill, ext, &layout.built_dir)
}

fn write_built_to(
    skill: &Skill,
    ext: Option<&extension::Extension>,
    built_dir: &Path,
) -> Result<()> {
    let dest = built_dir.join(&skill.name);
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("clearing built skill {}", dest.display()))?;
    }
    for rf in render::render(skill, ext) {
        let path = dest.join(&rf.rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &rf.contents).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// Render only selected skills below one workspace/actor cache directory.
/// No shared registry or frontend path is inspected or mutated.
pub(crate) fn render_workspace_capabilities(
    layout: &WorkspaceCapabilityLayout,
    sources: &Sources,
    plan: &crate::access::CapabilityPlan,
) -> Result<WorkspaceCapabilityReport> {
    let mut bundled = embed::bundled_skills();
    let mut rendered = Vec::new();
    for source in plan.skills.available_sources() {
        let skill = match source {
            crate::access::ResolvedSkillSource::Bundled { name } => {
                let index = bundled
                    .iter()
                    .position(|skill| skill.name == name)
                    .ok_or_else(|| anyhow::anyhow!("bundled skill `{name}` disappeared"))?;
                bundled.swap_remove(index)
            }
            crate::access::ResolvedSkillSource::Machine { name, path } => {
                plugin::load_exact(&name, &path)
                    .with_context(|| format!("loading exact machine skill `{name}`"))?
            }
        };
        let ext = sources
            .extensions_dir
            .as_deref()
            .and_then(|directory| extension::load(&skill.name, directory));
        write_fresh_built_to(&skill, ext.as_ref(), &layout.built_dir)?;
        rendered.push(skill.name);
    }
    Ok(WorkspaceCapabilityReport {
        rendered_dir: layout.built_dir.clone(),
        rendered,
    })
}

fn write_fresh_built_to(
    skill: &Skill,
    ext: Option<&extension::Extension>,
    built_dir: &Path,
) -> Result<()> {
    let dest = built_dir.join(&skill.name);
    match fs::symlink_metadata(&dest) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(_) => anyhow::bail!(
            "workspace capability skill destination appeared during render: {}",
            dest.display()
        ),
    }
    for rendered in render::render(skill, ext) {
        let path = dest.join(&rendered.rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &rendered.contents)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn create_symlink(link: &Link) -> Result<()> {
    if let Some(parent) = link.link_path.parent() {
        fs::create_dir_all(parent)?;
    }
    remove_existing(&link.link_path)?;
    std::os::unix::fs::symlink(&link.target, &link.link_path).with_context(|| {
        format!(
            "linking {} → {}",
            link.link_path.display(),
            link.target.display()
        )
    })
}

/// Remove whatever sits at `path` (symlink, file, or dir). `symlink_metadata`
/// does not follow the link, so a dangling symlink is handled too.
fn remove_existing(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn sandbox() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("brain-skills-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn sync_writes_built_skill_and_registry_and_frontend_links() {
        let root = sandbox();
        let layout = Layout::under_root(&root);
        let report = sync(&layout, &Sources::default()).unwrap();
        assert!(report.installed.iter().any(|n| n == "article-summarizer"));

        let built = layout.built_dir.join("article-summarizer").join("SKILL.md");
        assert!(built.is_file());
        assert!(
            fs::read_to_string(&built)
                .unwrap()
                .contains("article-summarizer")
        );

        let registry = layout.agents_dir.join("article-summarizer");
        assert_eq!(
            fs::read_link(&registry).unwrap(),
            layout.built_dir.join("article-summarizer")
        );
        for f in &layout.frontends {
            let fe = f.join("article-summarizer");
            assert_eq!(fs::read_link(&fe).unwrap(), registry);
            assert!(fe.join("SKILL.md").is_file());
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_installs_plugins_alongside_bundled_skills() {
        let root = sandbox();
        let plugins = root.join("plugins");
        let p = plugins.join("my-plugin");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("SKILL.md"), "# my plugin").unwrap();

        let layout = Layout::under_root(&root);
        let sources = Sources {
            plugins_dir: Some(plugins),
            ..Sources::default()
        };
        let report = sync(&layout, &sources).unwrap();
        assert!(report.installed.iter().any(|n| n == "my-plugin"));
        assert!(
            layout
                .agents_dir
                .join("my-plugin")
                .join("SKILL.md")
                .is_file()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_injects_an_extension_into_a_plugin_skill_md() {
        let root = sandbox();
        // A plugin with an extension hook, plus an extension file for it.
        let plugins = root.join("plugins");
        let p = plugins.join("hooked");
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join("SKILL.md"),
            "# H\n<!-- brain:ext hooked:start -->\nrest\n",
        )
        .unwrap();
        let exts = root.join("extensions");
        fs::create_dir_all(&exts).unwrap();
        fs::write(exts.join("hooked.md"), "[hooked:start]\nINJECTED\n").unwrap();

        let layout = Layout::under_root(&root);
        let sources = Sources {
            extensions_dir: Some(exts),
            plugins_dir: Some(plugins),
        };
        sync(&layout, &sources).unwrap();
        let built = fs::read_to_string(layout.built_dir.join("hooked").join("SKILL.md")).unwrap();
        assert!(built.contains("INJECTED"));
        assert!(!built.contains("brain:ext"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_is_idempotent() {
        let root = sandbox();
        let layout = Layout::under_root(&root);
        sync(&layout, &Sources::default()).unwrap();
        sync(&layout, &Sources::default()).unwrap();
        assert!(layout.agents_dir.join("article-summarizer").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
