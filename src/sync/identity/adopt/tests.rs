use super::*;

const LOCAL_ID: &str = "8d7d67d6-63fc-4d99-8ff9-ebe31ac93fed";
const PEER_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const INGRESS_ID: &str = "eefc2f33-a780-4425-a83c-235990668aef";

fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("fixed workspace UUID")
}

fn manifest_bytes(id: &str) -> Vec<u8> {
    format!(
        "{{\n  \"schema_version\": 1,\n  \"workspace_id\": \"{id}\",\n  \"receiver_ingress_id\": \"{INGRESS_ID}\",\n  \"minimum_brain_version\": \"0.1.0\"\n}}\n"
    )
    .into_bytes()
}

fn remote() -> Remote {
    Remote {
        env: Vec::new(),
        arg: "BRAIN:shared/brain".to_owned(),
    }
}

fn output(success: bool, stdout: &[u8], stderr: &str) -> RemoteCommandOutput {
    RemoteCommandOutput {
        success,
        stdout: stdout.to_vec(),
        stderr: stderr.to_owned(),
    }
}

fn responder(
    manifest: Option<Vec<u8>>,
) -> impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput {
    move |_, args| match args.first().map(String::as_str) {
        // B2 semantics: a missing object reads as success with no bytes.
        Some("cat") => manifest
            .clone()
            .map_or_else(|| output(true, b"", ""), |bytes| output(true, &bytes, "")),
        Some("lsf") => output(true, b"", ""),
        command => panic!("unexpected remote command {command:?}"),
    }
}

/// The whole point: the joining machine must end up with the *peer's*
/// `receiver_ingress_id`, not a fresh one. Minting locally looks successful and
/// silently forks portable identity, because the manifest is the one file
/// bisync never reconciles.
#[test]
fn a_matching_remote_manifest_is_adopted_byte_for_byte() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(LOCAL_ID);

    let adoption = adopt_remote_manifest_with(
        root.path(),
        workspace_id(LOCAL_ID),
        &remote(),
        responder(Some(bytes)),
    )
    .unwrap();

    assert_eq!(adoption, ManifestAdoption::Adopted);
    let adopted = WorkspaceManifest::load(root.path(), "0.1.0").expect("adopted manifest");
    assert_eq!(adopted.workspace_id(), workspace_id(LOCAL_ID));
    assert_eq!(
        adopted.receiver_ingress_id(),
        workspace_id(INGRESS_ID),
        "adoption must preserve the peer's receiver ingress identity"
    );
}

#[test]
fn an_empty_remote_leaves_minting_to_the_caller() {
    let root = tempfile::tempdir().unwrap();

    let adoption = adopt_remote_manifest_with(
        root.path(),
        workspace_id(LOCAL_ID),
        &remote(),
        responder(None),
    )
    .unwrap();

    assert_eq!(adoption, ManifestAdoption::RemoteHasNoManifest);
    assert!(!WorkspaceManifest::path(root.path()).exists());
}

#[test]
fn a_remote_owned_by_another_workspace_is_refused_without_writing() {
    let root = tempfile::tempdir().unwrap();

    let error = adopt_remote_manifest_with(
        root.path(),
        workspace_id(LOCAL_ID),
        &remote(),
        responder(Some(manifest_bytes(PEER_ID))),
    )
    .unwrap_err();

    assert!(error.to_string().contains(PEER_ID), "{error:#}");
    assert!(!WorkspaceManifest::path(root.path()).exists());
}

#[test]
fn a_malformed_remote_manifest_is_refused_without_writing() {
    let root = tempfile::tempdir().unwrap();

    let error = adopt_remote_manifest_with(
        root.path(),
        workspace_id(LOCAL_ID),
        &remote(),
        responder(Some(b"{ not json".to_vec())),
    )
    .unwrap_err();

    assert!(!WorkspaceManifest::path(root.path()).exists(), "{error:#}");
}

#[test]
fn an_existing_local_manifest_is_never_replaced() {
    let root = tempfile::tempdir().unwrap();
    WorkspaceManifest::new(workspace_id(LOCAL_ID))
        .write_new(root.path())
        .unwrap();
    let before = std::fs::read(WorkspaceManifest::path(root.path())).unwrap();

    let adoption = adopt_remote_manifest_with(
        root.path(),
        workspace_id(LOCAL_ID),
        &remote(),
        responder(Some(manifest_bytes(LOCAL_ID))),
    )
    .unwrap();

    assert_eq!(adoption, ManifestAdoption::AlreadyLocal);
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(root.path())).unwrap(),
        before
    );
}
