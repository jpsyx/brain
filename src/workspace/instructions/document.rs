use serde::{Deserialize, Serialize};
use std::ops::Range;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use super::{RemovedLegacySpan, legacy::remove_known_sections};

const START: &str = "<!-- brain:managed-workspace:start schema=1 -->";
const END: &str = "<!-- brain:managed-workspace:end -->";
const MARKER: &str = "brain:managed-workspace";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstructionReceipt {
    pub schema_version: u32,
    pub generated_revision: u32,
    pub body_sha256: String,
    /// Digest of the supplied rendering input. The package composer may replace
    /// this with its verified source-snapshot digest before publication.
    pub sources_sha256: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DocumentPlan {
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
    pub receipt: InstructionReceipt,
    pub changed: bool,
    pub removed_legacy: Vec<RemovedLegacySpan>,
}

pub(crate) fn plan_document(
    existing: Option<&[u8]>,
    desired_body: &str,
    prior: Option<&InstructionReceipt>,
) -> Result<DocumentPlan> {
    if let Some(receipt) = prior {
        validate_revision(receipt)?;
    }
    ensure!(
        marker_count(desired_body) == 0,
        "generated workspace instructions contain reserved managed markers"
    );
    let text = std::str::from_utf8(existing.unwrap_or_default())
        .context("workspace AGENTS.md is not valid UTF-8")?;
    let desired = format!("{START}\n{desired_body}\n{END}\n");
    let (after, removed_legacy) = if let Some(region) = parse_region(text)? {
        let body_hash = digest(text[region.body.clone()].as_bytes());
        let owned = prior.is_some_and(|receipt| receipt.body_sha256 == body_hash);
        ensure!(
            owned || text[region.entire.clone()] == desired,
            "workspace AGENTS.md managed region differs from its receipt and known render"
        );
        let mut after = text.as_bytes().to_vec();
        after.splice(region.entire, desired.bytes());
        (after, Vec::new())
    } else {
        let (mut after, removed) = remove_known_sections(text);
        if !after.is_empty() && !after.ends_with(b"\n") {
            after.push(b'\n');
        }
        after.extend_from_slice(desired.as_bytes());
        (after, removed)
    };
    Ok(DocumentPlan {
        before: existing.map(<[u8]>::to_vec),
        changed: existing != Some(after.as_slice()),
        after,
        receipt: InstructionReceipt {
            schema_version: 1,
            generated_revision: 2,
            body_sha256: digest(desired_body.as_bytes()),
            sources_sha256: digest(desired_body.as_bytes()),
        },
        removed_legacy,
    })
}

struct Region {
    entire: Range<usize>,
    body: Range<usize>,
}

fn parse_region(text: &str) -> Result<Option<Region>> {
    let count = marker_count(text);
    if count == 0 {
        return Ok(None);
    }
    ensure!(
        count == 2,
        "duplicate or incomplete workspace instruction markers"
    );
    let start = text
        .find(START)
        .context("invalid workspace instruction start marker")?;
    let end = text
        .find(END)
        .context("invalid workspace instruction end marker")?;
    let body_start = start + START.len() + 1;
    let body_end = end.saturating_sub(1);
    let region_end = end + END.len() + 1;
    ensure!(
        (start == 0 || text.as_bytes().get(start - 1) == Some(&b'\n'))
            && text.as_bytes().get(body_start - 1) == Some(&b'\n')
            && body_start <= body_end
            && text.as_bytes().get(body_end) == Some(&b'\n')
            && text.as_bytes().get(region_end - 1) == Some(&b'\n'),
        "malformed workspace instruction marker boundaries"
    );
    Ok(Some(Region {
        entire: start..region_end,
        body: body_start..body_end,
    }))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Verify currentness without claiming ownership of the surrounding user text.
/// Transaction publication must separately compare its complete before-image.
pub(crate) fn verified_body<'a>(
    document: &'a [u8],
    receipt: &InstructionReceipt,
) -> Result<&'a str> {
    validate_revision(receipt)?;
    let text = std::str::from_utf8(document).context("workspace AGENTS.md is not valid UTF-8")?;
    let region = parse_region(text)?.context("workspace AGENTS.md managed region is absent")?;
    let body = &text[region.body];
    ensure!(
        digest(body.as_bytes()) == receipt.body_sha256,
        "workspace AGENTS.md managed body differs from its receipt"
    );
    Ok(body)
}

fn validate_revision(receipt: &InstructionReceipt) -> Result<()> {
    ensure!(
        receipt.schema_version == 1 && (1..=2).contains(&receipt.generated_revision),
        "unsupported workspace instruction receipt schema or revision"
    );
    Ok(())
}

fn marker_count(text: &str) -> usize {
    text.split("<!--")
        .skip(1)
        .filter(|comment| comment.trim_start().starts_with(MARKER))
        .count()
}

/// Distinguish a missing owned region from a present malformed or modified one.
pub(crate) fn has_managed_region(document: &[u8]) -> Result<bool> {
    let text = std::str::from_utf8(document).context("workspace AGENTS.md is not valid UTF-8")?;
    Ok(parse_region(text)?.is_some())
}
