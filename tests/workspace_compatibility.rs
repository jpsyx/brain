use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use brain::workspace::{
    InteractionMode, MachineRegistry, ManifestError, ReadinessError, ReadinessField, RegistryError,
    WorkspaceId, WorkspaceManifest, WorkspaceName, WorkspaceRecord, readiness_action,
    validate_registry,
};
use serde_json::Map;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const INGRESS_ID: &str = "c48b0de2-361d-43aa-8e7d-9a60ba6caf39";

fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("fixed workspace UUID")
}

fn workspace_name(raw: &str) -> WorkspaceName {
    WorkspaceName::parse(raw).expect("fixed workspace name")
}

fn manifest_bytes(schema_version: u32, workspace_id: &str, minimum: &str) -> Vec<u8> {
    format!(
        "{{\"schema_version\":{schema_version},\"workspace_id\":\"{workspace_id}\",\"receiver_ingress_id\":\"{INGRESS_ID}\",\"minimum_brain_version\":\"{minimum}\"}}"
    )
    .into_bytes()
}

fn record(raw_id: &str, root: &str) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: workspace_id(raw_id),
        root: PathBuf::from(root),
        aliases: BTreeSet::new(),
        local_user_id: "pablo".to_owned(),
        receiver_enabled: false,
        env: Map::new(),
    }
}

#[test]
fn schema_one_uses_strict_numeric_minimum_version_compatibility() {
    let current = manifest_bytes(1, PERSONAL_ID, "0.16.0");
    assert!(WorkspaceManifest::parse(&current, "0.16.0").is_ok());
    assert!(WorkspaceManifest::parse(&current, "0.16.1").is_ok());

    let update_required =
        WorkspaceManifest::parse(&manifest_bytes(1, PERSONAL_ID, "0.16.0"), "0.15.9")
            .expect_err("older client must be refused");
    assert_eq!(
        update_required,
        ManifestError::IncompatibleBrainVersion {
            current: "0.15.9".to_owned(),
            minimum: "0.16.0".to_owned(),
        }
    );
    assert_eq!(
        update_required.to_string(),
        "workspace requires Brain 0.16.0 or newer; this is Brain 0.15.9"
    );
}

#[test]
fn malformed_versions_and_noncurrent_schemas_fail_closed() {
    for malformed_minimum in ["0.16", "0.16.x", "0.16.0.1"] {
        assert!(matches!(
            WorkspaceManifest::parse(&manifest_bytes(1, PERSONAL_ID, malformed_minimum), "0.16.0"),
            Err(ManifestError::InvalidMinimumBrainVersion { .. })
        ));
    }
    for malformed_current in ["0.16", "0.16.x", "0.16.0.1"] {
        assert!(matches!(
            WorkspaceManifest::parse(&manifest_bytes(1, PERSONAL_ID, "0.16.0"), malformed_current),
            Err(ManifestError::InvalidCurrentBrainVersion { .. })
        ));
    }
    for unsupported in [0, 99] {
        assert_eq!(
            WorkspaceManifest::parse(
                &manifest_bytes(unsupported, PERSONAL_ID, "0.16.0"),
                "0.16.0"
            ),
            Err(ManifestError::UnsupportedSchema {
                found: unsupported,
                supported: 1,
            })
        );
    }
}

#[test]
fn readiness_rejects_root_manifest_mismatch_and_internal_setup_stays_nonprompting() {
    let name = workspace_name("family");
    let record = record(PERSONAL_ID, "/brains/family");
    let mismatched =
        WorkspaceManifest::parse(&manifest_bytes(1, FAMILY_ID, "0.16.0"), "0.16.0").unwrap();
    assert_eq!(
        readiness_action(
            &name,
            &record,
            Ok(mismatched),
            InteractionMode::NonInteractive,
        ),
        Err(ReadinessError::WorkspaceIdMismatch {
            registry: PERSONAL_ID.to_owned(),
            manifest: FAMILY_ID.to_owned(),
        })
    );

    let missing = ManifestError::Io {
        operation: "read workspace manifest",
        path: PathBuf::from("/brains/family/.config/workspace.json"),
        kind: std::io::ErrorKind::NotFound,
        message: "not found".to_owned(),
    };
    assert_eq!(
        readiness_action(&name, &record, Err(missing), InteractionMode::Internal),
        Err(ReadinessError::Incomplete {
            canonical_name: "family".to_owned(),
            missing: vec![ReadinessField::Manifest],
            internal: true,
        })
    );
}

#[test]
fn duplicate_local_workspace_uuids_are_rejected() {
    let registry = MachineRegistry {
        schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
        default_workspace: workspace_name("personal"),
        workspaces: BTreeMap::from([
            (
                workspace_name("personal"),
                record(PERSONAL_ID, "/brains/personal"),
            ),
            (
                workspace_name("family"),
                record(PERSONAL_ID, "/brains/family"),
            ),
        ]),
        env: serde_json::Map::new(),
    };

    assert_eq!(
        validate_registry(&registry),
        Err(RegistryError::DuplicateWorkspaceId {
            workspace_id: workspace_id(PERSONAL_ID),
        })
    );
}
