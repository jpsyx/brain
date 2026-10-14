//! Render sources and publish managed skills through the recoverable installer.
//! Selected capability rendering remains isolated in each actor cache.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::install_transaction::{
    Currentness, InstallIdentity, InstallReceipt, InstallVersion, LockedInstall, NoStamp,
    RenderedInstall, StampStore,
};
use super::layout::{Layout, WorkspaceCapabilityLayout};
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
#[derive(Debug)]
pub struct Report {
    pub installed: Vec<String>,
    /// Skills whose generated files and eligible frontend links were removed.
    /// Unowned residual content is listed separately in `retained`.
    pub pruned: Vec<String>,
    pub retained: Vec<String>,
}

/// What one cache-local selected skill render produced.
pub struct WorkspaceCapabilityReport {
    pub rendered_dir: PathBuf,
    pub rendered: Vec<String>,
}

/// Render + install every bundled skill and plugin into `layout`, injecting
/// extensions from `sources`. Idempotent.
pub fn sync(layout: &Layout, sources: &Sources) -> Result<Report> {
    let root = layout
        .agents_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("invalid skills layout"))?;
    let expected = Layout::under_root(root);
    anyhow::ensure!(
        layout.agents_dir == layout.built_dir && layout.frontends == expected.frontends,
        "unsupported skills destinations"
    );
    sync_identity(&InstallIdentity::sandbox(root)?, sources, &mut NoStamp)
}

pub(crate) fn sync_workspace(
    workspace: &crate::workspace::WorkspaceContext,
    sources: &Sources,
) -> Result<Report> {
    struct WorkspaceStamp<'a>(&'a crate::workspace::WorkspaceContext);
    impl StampStore for WorkspaceStamp<'_> {
        fn write(&mut self, receipt: &InstallReceipt) -> Result<()> {
            crate::state::Db::open(self.0)?
                .set_skills_synced_version(&receipt.version.binary_version)
        }
    }
    sync_identity(
        &InstallIdentity::workspace(workspace)?,
        sources,
        &mut WorkspaceStamp(workspace),
    )
}

pub(crate) fn sync_identity(
    identity: &InstallIdentity,
    sources: &Sources,
    stamp: &mut dyn StampStore,
) -> Result<Report> {
    let mut locked = LockedInstall::acquire(identity)?;
    if let Some(recovered) = locked.recover(stamp)? {
        if recovered.receipt.version == InstallVersion::current() {
            return Ok(recovered.report);
        }
    }
    let version = InstallVersion::current();
    if let Currentness::Repairable(missing) = locked.inspect(&version)? {
        crate::logging::log(format!(
            "skills repairing {} missing or noncanonical artifacts",
            missing.len()
        ));
    }
    let rendered = render_sources(sources)?;
    let report = locked.preflight(&rendered, &version)?;
    locked.install(rendered, &version, stamp)?;
    Ok(report)
}

pub(crate) fn render_sources(sources: &Sources) -> Result<RenderedInstall> {
    let mut skills = embed::bundled_skills();
    if let Some(dir) = &sources.plugins_dir {
        skills.extend(plugin::discover_checked(dir)?);
    }
    let mut output = RenderedInstall::new();
    for skill in skills {
        anyhow::ensure!(
            !output.contains_key(&skill.name),
            "core/plugin skill name collision: {}",
            skill.name
        );
        let ext = sources
            .extensions_dir
            .as_deref()
            .map(|dir| extension::load_checked(&skill.name, dir))
            .transpose()?
            .flatten();
        let mut files = std::collections::BTreeMap::new();
        for file in render::render(&skill, ext.as_ref()) {
            let name = file
                .rel_path
                .into_os_string()
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF-8 rendered path"))?;
            anyhow::ensure!(
                files.insert(name, file.contents).is_none(),
                "duplicate rendered skill path"
            );
        }
        output.insert(skill.name, files);
    }
    Ok(output)
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
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sync_writes_workspace_skill_and_frontend_links() {
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

        for f in &layout.frontends {
            let fe = f.join("article-summarizer");
            assert_eq!(
                fs::read_link(&fe).unwrap(),
                PathBuf::from("../../.agents/skills/article-summarizer")
            );
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
    fn sync_preserves_and_links_user_skills_already_in_agents_directory() {
        let root = sandbox();
        let user_skill = root.join(".agents/skills/my-skill");
        fs::create_dir_all(&user_skill).unwrap();
        fs::write(user_skill.join("SKILL.md"), "# user skill").unwrap();

        let layout = Layout::under_root(&root);
        let report = sync(&layout, &Sources::default()).unwrap();

        assert!(report.installed.iter().any(|name| name == "my-skill"));
        assert_eq!(
            fs::read_link(root.join(".claude/skills/my-skill")).unwrap(),
            PathBuf::from("../../.agents/skills/my-skill")
        );
        assert_eq!(
            fs::read_to_string(user_skill.join("SKILL.md")).unwrap(),
            "# user skill"
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
    fn sync_prunes_a_rendered_skill_whose_plugin_disappeared() {
        let root = sandbox();
        let plugins = root.join("plugins");
        let gone = plugins.join("renamed-away");
        fs::create_dir_all(&gone).unwrap();
        fs::write(gone.join("SKILL.md"), "# old name").unwrap();

        let layout = Layout::under_root(&root);
        let sources = Sources {
            plugins_dir: Some(plugins),
            ..Sources::default()
        };
        sync(&layout, &sources).unwrap();
        assert!(layout.agents_dir.join("renamed-away").is_dir());

        fs::remove_dir_all(&gone).unwrap();
        let report = sync(&layout, &sources).unwrap();

        assert!(!report.installed.iter().any(|n| n == "renamed-away"));
        assert!(report.pruned.iter().any(|n| n == "renamed-away"));
        assert!(!layout.agents_dir.join("renamed-away").exists());
        for frontend in &layout.frontends {
            assert!(
                fs::symlink_metadata(frontend.join("renamed-away")).is_err(),
                "frontend link must be pruned too"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_never_removes_a_user_authored_workspace_skill() {
        let root = sandbox();
        let mine = root.join(".agents/skills/hand-written");
        fs::create_dir_all(&mine).unwrap();
        fs::write(mine.join("SKILL.md"), "# mine").unwrap();
        let plugins = root.join("plugins");
        let gone = plugins.join("temporary");
        fs::create_dir_all(&gone).unwrap();
        fs::write(gone.join("SKILL.md"), "# temp").unwrap();

        let layout = Layout::under_root(&root);
        let sources = Sources {
            plugins_dir: Some(plugins),
            ..Sources::default()
        };
        sync(&layout, &sources).unwrap();
        fs::remove_dir_all(&gone).unwrap();
        let report = sync(&layout, &sources).unwrap();

        assert!(report.installed.iter().any(|n| n == "hand-written"));
        assert!(!report.pruned.iter().any(|n| n == "hand-written"));
        assert_eq!(fs::read_to_string(mine.join("SKILL.md")).unwrap(), "# mine");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_preserves_unproven_dangling_user_skill_links() {
        let root = sandbox();
        let mine = root.join(".agents/skills/short-lived");
        fs::create_dir_all(&mine).unwrap();
        fs::write(mine.join("SKILL.md"), "# mine").unwrap();

        let layout = Layout::under_root(&root);
        sync(&layout, &Sources::default()).unwrap();
        fs::remove_dir_all(&mine).unwrap();
        sync(&layout, &Sources::default()).unwrap();

        for frontend in &layout.frontends {
            assert!(
                fs::symlink_metadata(frontend.join("short-lived")).is_ok(),
                "unproven user link must be preserved"
            );
        }
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
