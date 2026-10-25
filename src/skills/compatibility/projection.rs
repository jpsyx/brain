use super::{ExtensionOrigin, ExtensionProjection, hooks, provenance};
use crate::skills::extension::Extension;
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

const FOCUSED: &[&str] = &[
    "workspace",
    "agenda",
    "projects",
    "process-capture",
    "organize",
    "brain-sync",
];
type Definitions = BTreeMap<String, (String, String)>;

pub(super) fn current(sources: &BTreeMap<String, String>) -> Result<ExtensionProjection> {
    let mut projection = ExtensionProjection::default();
    let mut definitions = Definitions::new();
    let workspace_hooks: BTreeSet<_> = sources
        .iter()
        .filter(|(path, _)| owner(path).ok() == Some("workspace"))
        .flat_map(|(_, text)| {
            text.lines()
                .filter_map(crate::skills::extension::section_header)
        })
        .map(str::to_owned)
        .collect();
    let mut workspace_catch_all = String::new();
    for (path, text) in sources {
        let source_owner = owner(path)?;
        let blocks = provenance::blocks(text)
            .map_err(|error| anyhow::anyhow!("extension {path}: {error}"))?;
        projection
            .sources
            .insert(path.clone(), text.as_bytes().to_vec());
        for block in blocks {
            let content = text[block.range.clone()].trim();
            let mapping = block
                .hook
                .as_deref()
                .and_then(|hook| hooks::mapping(source_owner, hook));
            let workspace_alias = mapping.is_none()
                && source_owner == "second-brain"
                && block.hook.as_ref().is_some_and(|hook| {
                    hook.starts_with("workspace:") || workspace_hooks.contains(hook)
                });
            let catch_alias = (source_owner == "workspace" && block.hook.is_none())
                || (matches!(source_owner, "workspace" | "second-brain")
                    && block.hook.as_deref() == Some("workspace:catch-all"));
            let (destination, logical_hook) = if catch_alias {
                ("workspace", "workspace:catch-all".to_owned())
            } else if workspace_alias {
                ("workspace", block.hook.clone().unwrap_or_default())
            } else {
                mapping.map_or_else(
                    || {
                        (
                            source_owner,
                            block
                                .hook
                                .clone()
                                .unwrap_or_else(|| format!("{source_owner}:catch-all")),
                        )
                    },
                    |(_, canonical)| (hooks::skill(canonical), canonical.to_owned()),
                )
            };
            let legacy_context = mapping.is_none()
                && !workspace_alias
                && !catch_alias
                && matches!(source_owner, "second-brain" | "todo");
            let key = if legacy_context {
                format!("{source_owner}:legacy:{logical_hook}")
            } else {
                logical_hook.clone()
            };
            projection.origins.push(ExtensionOrigin {
                source_path: path.clone(),
                source_hook: block.hook.clone(),
                content_range: block.range,
                logical_hook: key,
                owner: if legacy_context && source_owner == "second-brain" {
                    "workspace".to_owned()
                } else {
                    destination.to_owned()
                },
            });
            if catch_alias {
                if accept(
                    &mut definitions,
                    "workspace/workspace:catch-all",
                    content,
                    path,
                )? {
                    content.clone_into(&mut workspace_catch_all);
                }
            } else if legacy_context || block.hook.is_none() {
                let target = match source_owner {
                    "second-brain" => &mut projection.workspace.catch_all,
                    "todo" => &mut projection.legacy_task_context,
                    other => {
                        &mut projection
                            .by_skill
                            .entry(other.to_owned())
                            .or_default()
                            .catch_all
                    }
                };
                append(target, content);
            } else if accept(
                &mut definitions,
                &format!("{destination}/{logical_hook}"),
                content,
                path,
            )? {
                let target = if destination == "workspace" {
                    &mut projection.workspace
                } else {
                    projection
                        .by_skill
                        .entry(destination.to_owned())
                        .or_default()
                };
                target.hooks.insert(logical_hook, content.to_owned());
            }
        }
    }
    append(&mut projection.workspace.catch_all, &workspace_catch_all);
    Ok(projection)
}

pub(super) fn project_down(
    sources: &BTreeMap<String, String>,
    projection: &mut ExtensionProjection,
) -> Result<()> {
    let mut patches = BTreeMap::new();
    for origin in &projection.origins {
        let text = &sources[&origin.source_path];
        if text[origin.content_range.clone()].trim().is_empty() {
            continue;
        }
        let source_owner = owner(&origin.source_path)?;
        let mapping = origin
            .source_hook
            .as_deref()
            .and_then(|hook| hooks::mapping(source_owner, hook));
        let legacy = if let Some((legacy, canonical)) = mapping {
            let legacy_owner = hooks::skill(legacy);
            if source_owner == legacy_owner {
                ensure!(
                    legacy == canonical || origin.source_hook.as_deref() == Some(legacy),
                    "cannot downgrade canonical hook {canonical} in legacy source {}; move it to {} before retrying",
                    origin.source_path,
                    Path::new(&origin.source_path)
                        .with_file_name(format!("{}.md", hooks::skill(canonical)))
                        .display()
                );
                continue;
            }
            legacy
        } else if source_owner == "workspace" {
            let hook = origin
                .source_hook
                .as_deref()
                .unwrap_or("workspace:catch-all");
            ensure!(
                hooks::mapping("second-brain", hook).is_none(),
                "cannot preserve always-on workspace hook {hook} from {} in the legacy second-brain source: the ID has a reserved legacy phase",
                origin.source_path
            );
            hook
        } else {
            ensure!(
                !FOCUSED.contains(&source_owner),
                "cannot preserve the narrower activation scope of extension {} on the legacy target",
                origin.source_path
            );
            continue;
        };
        let legacy_owner = if source_owner == "workspace" && mapping.is_none() {
            "second-brain"
        } else {
            hooks::skill(legacy)
        };
        let destination =
            Path::new(&origin.source_path).with_file_name(format!("{legacy_owner}.md"));
        provenance::plan_block(sources, &mut patches, origin, legacy, destination)?;
    }
    let mut target_sources = sources.clone();
    for patch in patches.values() {
        patch.validate_after(&patch.after)?;
        target_sources.insert(
            patch.relative_path.to_string_lossy().into_owned(),
            String::from_utf8(patch.after.clone())?,
        );
    }
    projection.by_skill.clear();
    projection.workspace = Extension::default();
    projection.legacy_task_context.clear();
    let mut definitions = Definitions::new();
    for (path, text) in &target_sources {
        let target_owner = owner(path)?;
        if FOCUSED.contains(&target_owner) {
            continue;
        }
        let target = projection
            .by_skill
            .entry(target_owner.to_owned())
            .or_default();
        for block in provenance::blocks(text)? {
            let content = text[block.range].trim();
            if let Some(hook) = block.hook {
                if accept(
                    &mut definitions,
                    &format!("{target_owner}/{hook}"),
                    content,
                    path,
                )? {
                    target.hooks.insert(hook, content.to_owned());
                }
            } else {
                append(&mut target.catch_all, content);
            }
        }
    }
    projection.legacy_source_patches = patches.into_values().collect();
    Ok(())
}

fn owner(path: &str) -> Result<&str> {
    let path_value = Path::new(path);
    ensure!(
        !path_value.as_os_str().is_empty()
            && path_value
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            && path_value
                .extension()
                .is_some_and(|extension| extension == "md"),
        "invalid relative extension source: {path}"
    );
    path_value
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid extension source: {path}"))
}

fn accept(definitions: &mut Definitions, key: &str, content: &str, path: &str) -> Result<bool> {
    if let Some((previous, source)) = definitions.get(key) {
        ensure!(
            previous.is_empty() || content.is_empty() || previous == content,
            "conflicting extension hook {key}: {source} and {path}"
        );
        if !previous.is_empty() {
            return Ok(false);
        }
    }
    definitions.insert(key.to_owned(), (content.to_owned(), path.to_owned()));
    Ok(true)
}

fn append(target: &mut String, content: &str) {
    if !content.is_empty() {
        if !target.is_empty() {
            target.push_str("\n\n");
        }
        target.push_str(content);
    }
}
