use super::ExtensionOrigin;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _, ops::Range, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionProvenance {
    pub source_path: String,
    pub source_sha256: String,
    pub logical_hook: String,
    pub inserted_sha256: String,
    pub source_range: Range<usize>,
    pub inserted_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourcePatch {
    pub relative_path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
    pub provenance: Vec<ProjectionProvenance>,
}

impl SourcePatch {
    pub(crate) fn validate_before(&self, current: Option<&[u8]>) -> Result<()> {
        ensure!(
            current == self.before.as_deref(),
            "extension source changed before projection: {}",
            self.relative_path.display()
        );
        Ok(())
    }

    pub(crate) fn validate_after(&self, current: &[u8]) -> Result<()> {
        ensure!(
            current == self.after,
            "projected extension source changed: {}",
            self.relative_path.display()
        );
        for origin in &self.provenance {
            let inserted = current
                .get(origin.inserted_range.clone())
                .ok_or_else(|| anyhow::anyhow!("invalid projected extension span"))?;
            ensure!(
                digest(inserted) == origin.inserted_sha256,
                "projected extension span changed: {}",
                origin.logical_hook
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct SourceBlock {
    pub hook: Option<String>,
    pub range: Range<usize>,
}

pub(super) fn blocks(text: &str) -> Result<Vec<SourceBlock>> {
    crate::skills::extension::parse_checked(text)?;
    let mut result = Vec::new();
    let mut hook = None;
    let mut start = 0;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some(name) = crate::skills::extension::section_header(line) {
            result.push(SourceBlock {
                hook,
                range: start..offset,
            });
            hook = Some(name.to_owned());
            start = offset + line.len();
        }
        offset += line.len();
    }
    result.push(SourceBlock {
        hook,
        range: start..text.len(),
    });
    Ok(result)
}

pub(super) fn plan_block(
    sources: &BTreeMap<String, String>,
    patches: &mut BTreeMap<PathBuf, SourcePatch>,
    origin: &ExtensionOrigin,
    legacy_hook: &str,
    destination: PathBuf,
) -> Result<()> {
    let source = &sources[&origin.source_path];
    let content = &source[origin.content_range.clone()];
    let before = sources
        .get(&destination.to_string_lossy().into_owned())
        .map(|text| text.as_bytes().to_vec());
    let current = patches.get(&destination).map_or_else(
        || before.as_deref().unwrap_or_default(),
        |patch| patch.after.as_slice(),
    );
    let text = std::str::from_utf8(current)?;
    let existing = blocks(text)?
        .into_iter()
        .find(|block| block.hook.as_deref() == Some(legacy_hook));
    let (index, addition) = if let Some(block) = existing {
        let previous = text[block.range.clone()].trim();
        ensure!(
            previous.is_empty() || previous == content.trim(),
            "conflicting extension hook {legacy_hook}: {} and {}",
            destination.display(),
            origin.source_path
        );
        if !previous.is_empty() {
            return Ok(());
        }
        let mut addition = terminated(content);
        if !current[..block.range.start].ends_with(b"\n") {
            addition.insert(0, '\n');
        }
        (block.range.start, addition)
    } else {
        let mut addition = String::new();
        if !current.is_empty() {
            if !current.ends_with(b"\n") {
                addition.push('\n');
            }
            addition.push('\n');
        }
        writeln!(addition, "[{legacy_hook}]")?;
        addition.push_str(&terminated(content));
        (current.len(), addition)
    };
    let patch = patches
        .entry(destination.clone())
        .or_insert_with(|| SourcePatch {
            relative_path: destination,
            after: before.clone().unwrap_or_default(),
            before,
            provenance: Vec::new(),
        });
    for previous in &mut patch.provenance {
        if previous.inserted_range.start >= index {
            previous.inserted_range.start += addition.len();
            previous.inserted_range.end += addition.len();
        } else {
            ensure!(
                previous.inserted_range.end <= index,
                "overlapping extension projection spans"
            );
        }
    }
    patch.after.splice(index..index, addition.bytes());
    patch.provenance.push(super::ProjectionProvenance {
        source_path: origin.source_path.clone(),
        source_sha256: digest(source.as_bytes()),
        logical_hook: origin.logical_hook.clone(),
        inserted_sha256: digest(addition.as_bytes()),
        source_range: origin.content_range.clone(),
        inserted_range: index..index + addition.len(),
    });
    Ok(())
}

fn terminated(content: &str) -> String {
    let mut text = content.to_owned();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
