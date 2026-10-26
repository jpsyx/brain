use crate::skills::{
    diagnostic::{Category, Diagnostic},
    install_transaction::package::JsonFieldPatch,
};
use anyhow::{Context as _, Result, ensure};
use serde_json::Value;
use std::collections::BTreeSet;

const SPLITS: &[(&str, &[&str])] = &[
    (
        "second-brain",
        &["projects", "process-capture", "organize", "brain-sync"],
    ),
    ("todo", &["agenda"]),
];

pub(super) fn expand(names: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut expanded: Vec<String> = names
        .iter()
        .filter(|name| seen.insert(name.to_ascii_lowercase()))
        .cloned()
        .collect();
    for name in names {
        if let Some((_, successors)) = SPLITS
            .iter()
            .find(|(legacy, _)| name.eq_ignore_ascii_case(legacy))
        {
            for successor in *successors {
                if seen.insert((*successor).to_owned()) {
                    expanded.push((*successor).to_owned());
                }
            }
        }
    }
    expanded
}

pub(super) fn project_allowed_skills(config: &Value) -> Result<Option<JsonFieldPatch>> {
    let Some(before) = configured_names(config)? else {
        return Ok(None);
    };
    let after = super::expand_legacy_names(&before);
    Ok((before != after).then(|| field_patch(&before, &after)))
}

pub(super) fn reverse_allowed_skills(
    config: &Value,
    history: &[JsonFieldPatch],
) -> Result<Option<JsonFieldPatch>> {
    let mut introduced = BTreeSet::new();
    for patch in history {
        ensure!(
            patch.relative_path == std::path::Path::new(".config/config.json")
                && patch.field == "allowed_skills",
            "invalid capability migration history path/field"
        );
        let before = patch
            .before
            .as_ref()
            .context("missing capability migration before value")?;
        let after = patch
            .after
            .as_ref()
            .context("missing capability migration after value")?;
        let before: Vec<String> = serde_json::from_value(before.clone())?;
        let after: Vec<String> = serde_json::from_value(after.clone())?;
        ensure!(
            super::expand_legacy_names(&before) == after,
            "invalid capability migration history expansion"
        );
        introduced.extend(after.into_iter().filter(|name| {
            !before
                .iter()
                .any(|original| original.eq_ignore_ascii_case(name))
        }));
    }
    let Some(before) = configured_names(config)? else {
        return Ok(None);
    };
    validate_legacy_names(&before, true)?;
    let after = before
        .iter()
        .filter(|name| !introduced.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    Ok((before != after).then(|| field_patch(&before, &after)))
}

/// Without a current package receipt, legacy-only selectors retain their legacy meaning.
pub(super) fn validate_legacy_allowed_skills(config: &Value) -> Result<()> {
    if let Some(names) = configured_names(config)? {
        validate_legacy_names(&names, false)?;
    }
    Ok(())
}

fn validate_legacy_names(names: &[String], current_generation: bool) -> Result<()> {
    let selected: BTreeSet<_> = names.iter().map(|name| name.to_ascii_lowercase()).collect();
    // A current receipt makes a legacy-only selector a narrowed modern choice.
    // Without that evidence, only canonical selectors require an equivalent umbrella.
    for (legacy, successors) in SPLITS {
        if (current_generation && selected.contains(*legacy))
            || successors.iter().any(|name| selected.contains(*name))
        {
            ensure!(
                selected.contains(*legacy)
                    && successors.iter().all(|name| selected.contains(*name)),
                Diagnostic::new(
                    Category::LegacyCapabilityScope,
                    "prepare_legacy_package",
                    Some(".config/config.json"),
                )
            );
        }
    }
    Ok(())
}

fn configured_names(config: &Value) -> Result<Option<Vec<String>>> {
    let object = config
        .as_object()
        .context("configuration is not an object")?;
    object
        .get("allowed_skills")
        .map(|value| serde_json::from_value(value.clone()).context("invalid allowed_skills list"))
        .transpose()
}

fn field_patch(before: &[String], after: &[String]) -> JsonFieldPatch {
    JsonFieldPatch {
        relative_path: ".config/config.json".into(),
        field: "allowed_skills".into(),
        before: Some(serde_json::json!(before)),
        after: Some(serde_json::json!(after)),
    }
}
