//! Authenticate canonical projection inputs independently of legacy destination bytes.
use super::{BTreeMap, Journal, Result, SourceOwnership, SourcePatch, ensure, snapshot};
use crate::skills::compatibility::{ProjectionDirection, project_extensions};

pub(super) fn observe(
    identity: &super::super::InstallIdentity,
    patch: SourcePatch,
) -> Result<SourceOwnership> {
    let mut inputs = BTreeMap::new();
    for origin in &patch.provenance {
        canonical_path(&origin.source_path)?;
        snapshot::check_parent(&identity.root, &origin.source_path)?;
        let bytes = snapshot::read_optional_file(&identity.root.join(&origin.source_path))?
            .ok_or_else(|| anyhow::anyhow!("canonical extension source missing"))?;
        ensure!(
            snapshot::digest(&bytes) == origin.source_sha256,
            "canonical extension source changed before publication"
        );
        inputs.insert(origin.source_path.clone(), bytes);
    }
    let owned = SourceOwnership { patch, inputs };
    validate(&owned)?;
    let destination = owned
        .patch
        .relative_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 projection destination"))?;
    snapshot::check_parent(&identity.root, destination)?;
    owned.patch.validate_before(
        snapshot::read_optional_file(&identity.root.join(destination))?.as_deref(),
    )?;
    Ok(owned)
}
fn canonical_path(path: &str) -> Result<()> {
    ensure!(
        [
            ".config/extensions/workspace.md",
            ".config/extensions/agenda.md",
            ".config/extensions/organize.md"
        ]
        .contains(&path),
        "unsupported canonical extension path"
    );
    Ok(())
}
pub(super) fn validate(owned: &SourceOwnership) -> Result<()> {
    ensure!(
        [
            std::path::Path::new(".config/extensions/todo.md"),
            std::path::Path::new(".config/extensions/second-brain.md")
        ]
        .contains(&owned.patch.relative_path.as_path()),
        "unsupported legacy extension destination"
    );
    ensure!(
        !owned.patch.provenance.is_empty(),
        "missing source projection provenance"
    );
    let mut sources = BTreeMap::new();
    for (path, bytes) in &owned.inputs {
        canonical_path(path)?;
        sources.insert(path.clone(), String::from_utf8(bytes.clone())?);
    }
    for origin in &owned.patch.provenance {
        let bytes = owned
            .inputs
            .get(&origin.source_path)
            .ok_or_else(|| anyhow::anyhow!("missing canonical projection source"))?;
        ensure!(
            snapshot::digest(bytes) == origin.source_sha256,
            "canonical source digest mismatch"
        );
    }
    if let Some(before) = &owned.patch.before {
        sources.insert(
            owned.patch.relative_path.to_string_lossy().into_owned(),
            String::from_utf8(before.clone())?,
        );
    }
    let expected = project_extensions(&sources, ProjectionDirection::Legacy105)?;
    ensure!(
        expected
            .legacy_source_patches
            .iter()
            .any(|patch| patch == &owned.patch),
        "source patch differs from supported projection"
    );
    owned.patch.validate_after(&owned.patch.after)
}

impl Journal {
    pub(in crate::skills::install_transaction) fn verify_sources(
        &self,
        root: &std::path::Path,
    ) -> Result<()> {
        for change in self.artifacts.values() {
            if let Some(source) = &change.source {
                for (path, bytes) in &source.inputs {
                    snapshot::check_parent(root, path)?;
                    ensure!(
                        snapshot::read_optional_file(&root.join(path))?.as_ref() == Some(bytes),
                        "canonical extension source changed during package publication: {path}"
                    );
                }
            }
        }
        Ok(())
    }
}
