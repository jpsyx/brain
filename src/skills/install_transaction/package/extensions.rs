//! Version-independent retained inputs, authenticated inside the package manifest.
use super::super::snapshot;
use crate::skills::compatibility::snapshot::CheckedSources;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtensionSnapshot {
    sources: BTreeMap<String, Vec<u8>>,
    sha256: String,
}

impl std::fmt::Debug for ExtensionSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ExtensionSnapshot(<redacted>)")
    }
}

impl ExtensionSnapshot {
    pub(crate) fn new(sources: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let bytes = serde_json::to_vec(&sources)?;
        snapshot::check_state_bytes(&bytes)?;
        let result = Self {
            sources,
            sha256: snapshot::digest(&bytes),
        };
        result.checked()?;
        Ok(result)
    }

    pub(crate) fn checked(&self) -> Result<CheckedSources> {
        let bytes = serde_json::to_vec(&self.sources)?;
        snapshot::check_state_bytes(&bytes)?;
        ensure!(
            snapshot::digest(&bytes) == self.sha256,
            "extension snapshot digest mismatch"
        );
        CheckedSources::from_snapshot(&self.sources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_inputs_reject_tampered_bytes_and_invalid_source_origins() {
        let sources =
            BTreeMap::from([(".config/extensions/custom.md".to_owned(), b"old".to_vec())]);
        let snapshot = ExtensionSnapshot::new(sources).unwrap();
        let mut tampered = serde_json::to_value(&snapshot).unwrap();
        tampered["sources"][".config/extensions/custom.md"] = serde_json::json!([110, 101, 119]);
        assert!(
            serde_json::from_value::<ExtensionSnapshot>(tampered)
                .unwrap()
                .checked()
                .is_err()
        );
        for path in [
            "../custom.md",
            "/external/custom.md",
            ".config/extensions/custom.txt",
        ] {
            assert!(
                ExtensionSnapshot::new(BTreeMap::from([(path.to_owned(), b"text".to_vec())]))
                    .is_err()
            );
        }
        assert!(
            ExtensionSnapshot::new(BTreeMap::from([("extensions/custom.md".into(), vec![255])]))
                .is_err()
        );
    }

    #[test]
    fn retained_snapshot_obeys_the_existing_state_byte_budget() {
        let sources = BTreeMap::from([(
            "extensions/custom.md".to_owned(),
            vec![b'x'; snapshot::STATE_FILE_LIMIT / 4],
        )]);
        assert!(ExtensionSnapshot::new(sources).is_err());
    }
}
