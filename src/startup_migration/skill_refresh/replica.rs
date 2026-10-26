//! Target-owned rendering in a one-workspace disposable replica.
use super::{
    sources::{self, Sources},
    target::{PrivateDirectory, Target},
};
use crate::{
    skills::install_transaction::RenderedInstall,
    workspace::{WorkspaceName, WorkspaceRecord},
};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn render(
    target: &Target,
    name: &WorkspaceName,
    record: &WorkspaceRecord,
    input: &Sources,
    customization: bool,
) -> Result<RenderedInstall> {
    let private = PrivateDirectory::new()?;
    let root = private.path().join("workspace");
    input.copy_to(&root, customization)?;
    let registry = serde_json::json!({"schema_version":4, "default_workspace":name.as_str(), "env":{}, "workspaces": {
        name.as_str(): {"workspace_id":record.workspace_id, "root":root, "local_user_id":record.local_user_id, "receiver_enabled": false, "env":{}}
    }});
    let path = private.path().join("xdg-config/brain/env.json");
    fs::create_dir_all(path.parent().expect("registry parent"))?;
    fs::write(path, serde_json::to_vec(&registry)?)?;
    target.run(&private, &["-w", name.as_str(), "skills", "sync"])?;
    let generated = root.join(".agents/skills");
    let mut output = RenderedInstall::new();
    for entry in fs::read_dir(&generated)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_dir(),
            "target produced a non-directory skill"
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid generated skill name"))?;
        let mut files = BTreeMap::new();
        sources::walk(
            &entry.path(),
            Path::new(""),
            &mut files,
            &mut Vec::new(),
            false,
        )?;
        ensure!(
            files.contains_key("SKILL.md"),
            "target omitted skill entrypoint"
        );
        ensure!(
            files
                .get(".brain-rendered")
                .and_then(|b| std::str::from_utf8(b).ok())
                .is_some_and(|v| v.trim() == target.version),
            "target marker version mismatch"
        );
        output.insert(name, files);
    }
    ensure!(!output.is_empty(), "target produced no skills");
    for frontend in [".claude/skills", ".codex/skills", ".opencode/skills"] {
        let mut names = Vec::new();
        for entry in fs::read_dir(root.join(frontend))? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid frontend name"))?;
            ensure!(
                output.contains_key(&name) && entry.file_type()?.is_symlink(),
                "unexpected target frontend entry"
            );
            let destination = fs::read_link(entry.path())?;
            let resolved = entry
                .path()
                .parent()
                .expect("frontend parent")
                .join(destination)
                .canonicalize()?;
            ensure!(
                resolved == generated.join(&name),
                "target frontend link escapes generated skill"
            );
            names.push(name);
        }
        ensure!(
            names.len() == output.len(),
            "target omitted frontend skill link"
        );
    }
    Ok(output)
}

pub(super) fn validate(
    core: &RenderedInstall,
    output: &RenderedInstall,
    sources: &Sources,
    target_version: &str,
) -> Result<()> {
    validate_plugins(core, sources)?;
    let expected = if target_version == crate::skills::current_version() {
        current_expected(sources)?
    } else {
        legacy_expected(core, sources)?
    };
    let mut actual = output.clone();
    for files in actual.values_mut() {
        files.remove(".brain-rendered");
    }
    ensure!(
        actual == expected,
        "target generated incomplete or unexpected skill content"
    );
    Ok(())
}

fn legacy_expected(core: &RenderedInstall, sources: &Sources) -> Result<RenderedInstall> {
    let mut expected = core.clone();
    let prefix = ".config/plugins/";
    for (path, bytes) in &sources.files {
        let Some(relative) = path.strip_prefix(prefix) else {
            continue;
        };
        let (name, relative) = relative
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("plugin files must belong to a skill directory"))?;
        ensure!(
            relative != ".brain-rendered",
            "plugin uses reserved rendered marker"
        );
        expected
            .entry(name.into())
            .or_default()
            .insert(relative.into(), bytes.clone());
    }
    // The audited renderer applies extensions only to SKILL.md, with this unchanged parser.
    for (name, files) in &mut expected {
        ensure!(files.contains_key("SKILL.md"), "plugin omitted SKILL.md");
        if let Some(extension) = sources.files.get(&format!(".config/extensions/{name}.md")) {
            let extension = crate::skills::extension::parse(std::str::from_utf8(extension)?);
            let body =
                String::from_utf8_lossy(files.get("SKILL.md").expect("validated entrypoint"));
            files.insert(
                "SKILL.md".into(),
                crate::skills::extension::apply(&body, &extension).into_bytes(),
            );
        }
        files.remove(".brain-rendered");
    }
    Ok(expected)
}

fn current_expected(sources: &Sources) -> Result<RenderedInstall> {
    use crate::skills::{
        compatibility::snapshot::CheckedSources,
        model::{Skill, SkillFile},
    };
    let mut catalog = crate::skills::embed::bundled_skills();
    let mut plugins: BTreeMap<String, Vec<SkillFile>> = BTreeMap::new();
    let mut extensions = BTreeMap::new();
    for (path, bytes) in &sources.files {
        if let Some(relative) = path.strip_prefix(".config/plugins/") {
            let (name, relative) = relative
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("plugin files must belong to a skill directory"))?;
            ensure!(
                relative != ".brain-rendered",
                "plugin uses reserved rendered marker"
            );
            plugins.entry(name.to_owned()).or_default().push(SkillFile {
                rel_path: relative.into(),
                contents: bytes.clone(),
            });
        }
        if path
            .strip_prefix(".config/extensions/")
            .is_some_and(|relative| {
                !relative.contains('/')
                    && Path::new(relative)
                        .extension()
                        .is_some_and(|ext| ext == "md")
            })
        {
            extensions.insert(path.clone(), bytes.clone());
        }
    }
    for (name, files) in plugins {
        ensure!(
            files
                .iter()
                .any(|file| file.rel_path == Path::new("SKILL.md")),
            "plugin omitted SKILL.md"
        );
        catalog.push(Skill {
            name,
            files,
            bundled: false,
        });
    }
    Ok(crate::skills::render::compile_catalog(
        &catalog,
        CheckedSources::from_snapshot(&extensions)?,
    )?
    .skills)
}

pub(super) fn validate_plugins(core: &RenderedInstall, sources: &Sources) -> Result<()> {
    for path in sources.files.keys() {
        if let Some(relative) = path.strip_prefix(".config/plugins/") {
            if let Some((name, _)) = relative.split_once('/') {
                ensure!(
                    !core.contains_key(name),
                    crate::skills::diagnostic::Diagnostic::new(
                        crate::skills::diagnostic::Category::CorePluginOverride,
                        "validate_target_plugins",
                        Some(&format!(".config/plugins/{name}"))
                    )
                );
            }
        }
    }
    Ok(())
}
