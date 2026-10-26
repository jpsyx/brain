//! Rendering a skill into the files to install.
//!
//! The base skill's `SKILL.md` is injected with the user's extension (if any)
//! via `extension::apply`, producing a **new built copy** — the repo/plugin
//! source is never mutated. All other files pass through unchanged.

use std::path::{Path, PathBuf};

use super::extension::{self, Extension};
use super::model::Skill;

/// A file to write into the built skill dir.
pub struct RenderedFile {
    pub rel_path: PathBuf,
    pub contents: Vec<u8>,
}

/// Render `skill` to its installable files, injecting `ext` into `SKILL.md`.
#[must_use]
pub fn render(skill: &Skill, ext: Option<&Extension>) -> Vec<RenderedFile> {
    skill
        .files
        .iter()
        .map(|f| {
            let contents = if is_skill_md(&f.rel_path) {
                let text = String::from_utf8_lossy(&f.contents);
                let body = ext.map_or_else(
                    || {
                        if skill.bundled {
                            extension::apply(&text, &Extension::default()).into_bytes()
                        } else {
                            f.contents.clone()
                        }
                    },
                    |e| extension::apply(&text, e).into_bytes(),
                );
                if skill.bundled {
                    add_prelude(&String::from_utf8_lossy(&body)).into_bytes()
                } else {
                    body
                }
            } else {
                f.contents.clone()
            };
            RenderedFile {
                rel_path: f.rel_path.clone(),
                contents,
            }
        })
        .collect()
}

fn is_skill_md(rel: &Path) -> bool {
    rel == Path::new("SKILL.md")
}

const PRELUDE: &str = "Resolve the selected Brain workspace and read its `AGENTS.md` if it has not already been loaded.\n\n";
const LEGACY_REFERENCE: &str = "references/legacy-task-context.md";

pub(crate) fn compile(
    skills: &[Skill],
    checked: &super::compatibility::snapshot::CheckedSources,
) -> anyhow::Result<super::install_transaction::RenderedInstall> {
    let projection = &checked.projection;
    let mut output = super::install_transaction::RenderedInstall::new();
    for skill in skills {
        let mut markers = std::collections::BTreeSet::new();
        for file in skill
            .files
            .iter()
            .filter(|file| skill.bundled && is_skill_md(&file.rel_path))
        {
            let text = std::str::from_utf8(&file.contents)?;
            for hook in text.lines().filter_map(extension::marker_hook) {
                anyhow::ensure!(
                    markers.insert(hook),
                    "duplicate extension marker {hook} in {}",
                    skill.name
                );
            }
        }
        anyhow::ensure!(
            !output.contains_key(&skill.name),
            super::diagnostic::Diagnostic::new(
                super::diagnostic::Category::CorePluginOverride,
                "render_sources",
                Some(&format!(".config/plugins/{}", skill.name))
            )
        );
        let ext =
            projection
                .by_skill
                .get(&skill.name)
                .cloned()
                .map(|mut ext| {
                    for (hook, content) in &mut ext.hooks {
                        if let Some(origin) = projection.origins.iter().find(|origin| {
                            origin.owner == skill.name
                                && origin.logical_hook == *hook
                                && std::str::from_utf8(
                                    &projection.sources[&origin.source_path]
                                        [origin.content_range.clone()],
                                )
                                .is_ok_and(|raw| !raw.trim().is_empty() && raw.trim() == content)
                        }) {
                            *content = with_skill_source(content, &origin.source_path);
                        }
                    }
                    if !ext.catch_all.is_empty() {
                        if let Some(origin) = projection.origins.iter().find(|origin| {
                            origin.owner == skill.name && origin.source_hook.is_none()
                        }) {
                            ext.catch_all = with_skill_source(&ext.catch_all, &origin.source_path);
                        }
                    }
                    ext
                });
        let mut files = std::collections::BTreeMap::new();
        for file in render(skill, ext.as_ref()) {
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
        if skill.bundled
            && matches!(skill.name.as_str(), "todo" | "agenda")
            && !projection.legacy_task_context.is_empty()
        {
            use sha2::Digest as _;
            let identity = format!(
                "{:x}",
                sha2::Sha256::digest(projection.legacy_task_context.as_bytes())
            );
            let mut reference = format!(
                "# Legacy task context\n\nContext ID: `sha256:{identity}`. Reuse this already-loaded context across skill handoffs. Do not rerun completed side effects.\n\n"
            );
            for origin in projection
                .origins
                .iter()
                .filter(|origin| origin.logical_hook.starts_with("todo:legacy:"))
            {
                let content = std::str::from_utf8(
                    &projection.sources[&origin.source_path][origin.content_range.clone()],
                )?
                .trim();
                if !content.is_empty() {
                    reference.push_str(&with_skill_source(content, &origin.source_path));
                    reference.push_str("\n\n");
                }
            }
            anyhow::ensure!(
                files
                    .insert(LEGACY_REFERENCE.to_owned(), reference.into_bytes())
                    .is_none(),
                "legacy reference destination collision"
            );
            let body = files
                .get_mut("SKILL.md")
                .ok_or_else(|| anyhow::anyhow!("bundled skill is missing SKILL.md"))?;
            body.extend_from_slice(format!("\n\nLoad [{LEGACY_REFERENCE}]({LEGACY_REFERENCE}) when its context ID has not already been loaded in this run.\n").as_bytes());
        }
        output.insert(skill.name.clone(), files);
    }
    Ok(output)
}

fn with_skill_source(content: &str, source: &str) -> String {
    if Path::new(source).parent() == Some(Path::new("extensions")) {
        return format!(
            "Customization source: `{source}`. Resolve its relative resource paths from `../../../extensions` relative to the owning installed `SKILL.md` directory, including when reading a generated reference file.\n\n{content}"
        );
    }
    with_source(content, source)
}

fn with_source(content: &str, source: &str) -> String {
    let base = Path::new(source).parent().unwrap_or_else(|| Path::new("."));
    format!(
        "Customization source: `{source}`. Resolve its relative resource paths from `{}` in the selected workspace.\n\n{content}",
        base.display()
    )
}

fn add_prelude(body: &str) -> String {
    if let Some(frontmatter) = body.strip_prefix("---\n") {
        if let Some(end) = frontmatter.find("\n---\n") {
            let split = 4 + end + 5;
            return format!("{}\n{PRELUDE}{}", &body[..split], &body[split..]);
        }
    }
    format!("{PRELUDE}{body}")
}

pub(crate) fn compile_catalog(
    skills: &[Skill],
    checked: super::compatibility::snapshot::CheckedSources,
) -> anyhow::Result<super::model::CompiledSkills> {
    use sha2::{Digest as _, Sha256};
    let output = compile(skills, &checked)?;
    let contract = crate::workspace::instructions::render_contract();
    let mut hasher = Sha256::new();
    for bytes in [
        b"brain-checked-extension-compiler:2".as_slice(),
        contract.as_bytes(),
        super::current_version().as_bytes(),
    ] {
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    for (path, bytes) in &checked.projection.sources {
        for input in [path.as_bytes(), bytes.as_slice()] {
            hasher.update((input.len() as u64).to_be_bytes());
            hasher.update(input);
        }
    }
    let workspace_body = render_workspace(&contract, &checked.projection);
    Ok(super::model::CompiledSkills {
        skills: output,
        projection: checked.projection,
        workspace_body,
        sources_sha256: format!("{:x}", hasher.finalize()),
    })
}

fn render_workspace(
    contract: &str,
    projection: &super::compatibility::ExtensionProjection,
) -> String {
    let mut body = contract.to_owned();
    for (hook, content) in &projection.workspace.hooks {
        if content.is_empty() {
            continue;
        }
        let source = projection.origins.iter().find(|origin| {
            origin.owner == "workspace"
                && origin.logical_hook == *hook
                && std::str::from_utf8(
                    &projection.sources[&origin.source_path][origin.content_range.clone()],
                )
                .is_ok_and(|raw| !raw.trim().is_empty() && raw.trim() == content)
        });
        body.push_str("\n\n## Workspace customization\n\n");
        body.push_str(&source.map_or_else(
            || content.clone(),
            |origin| with_source(content, &origin.source_path),
        ));
    }
    if !projection.workspace.catch_all.is_empty() {
        body.push_str("\n\n## Legacy workspace customizations\n\n");
        let source = projection.origins.iter().find(|origin| {
            origin.owner == "workspace"
                && std::str::from_utf8(
                    &projection.sources[&origin.source_path][origin.content_range.clone()],
                )
                .is_ok_and(|raw| {
                    !raw.trim().is_empty() && projection.workspace.catch_all.contains(raw.trim())
                })
        });
        body.push_str(&source.map_or_else(
            || projection.workspace.catch_all.clone(),
            |origin| with_source(&projection.workspace.catch_all, &origin.source_path),
        ));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::model::SkillFile;

    fn skill_md(body: &str) -> Skill {
        Skill {
            name: "t".to_owned(),
            bundled: false,
            files: vec![SkillFile {
                rel_path: PathBuf::from("SKILL.md"),
                contents: body.as_bytes().to_vec(),
            }],
        }
    }

    #[test]
    fn render_without_extension_is_passthrough() {
        let s = skill_md("# hi\nbody\n");
        let out = render(&s, None);
        assert_eq!(out[0].contents, b"# hi\nbody\n");
    }

    #[test]
    fn render_with_extension_injects_into_skill_md() {
        let s = skill_md("# T\n<!-- brain:ext t:start -->\nsteps\n");
        let mut ext = Extension::default();
        ext.hooks.insert("t:start".to_owned(), "FIRST".to_owned());
        let out = render(&s, Some(&ext));
        let text = String::from_utf8(out[0].contents.clone()).unwrap();
        assert!(text.contains("FIRST"));
        assert!(!text.contains("brain:ext"));
    }
}
