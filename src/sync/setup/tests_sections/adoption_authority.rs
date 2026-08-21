
#[test]
fn adoption_authority_requires_the_exact_selected_workspace_uuid() {
    assert_eq!(
        adoption_authorization(local_workspace_id(), None).unwrap(),
        AdoptionAuthorization::NeedsInteractiveConfirmation
    );
    assert_eq!(
        adoption_authorization(local_workspace_id(), Some(LOCAL_WORKSPACE_ID)).unwrap(),
        AdoptionAuthorization::Authorized
    );

    let mismatch =
        adoption_authorization(local_workspace_id(), Some(OTHER_WORKSPACE_ID)).unwrap_err();
    assert!(mismatch.to_string().contains(LOCAL_WORKSPACE_ID));
    assert!(mismatch.to_string().contains(OTHER_WORKSPACE_ID));
    let malformed = adoption_authorization(local_workspace_id(), Some("not-a-uuid"))
        .expect_err("malformed authority must fail closed");
    assert!(malformed.to_string().contains("valid workspace UUID"));
}

#[test]
fn identity_summary_names_the_local_workspace_target_and_observed_remote_state() {
    let local_name = crate::workspace::WorkspaceName::parse("family").unwrap();
    let target = "BRAIN:shared/brain";
    let manifestless = format_identity_summary(
        &local_name,
        local_workspace_id(),
        target,
        &crate::sync::identity::RemoteIdentityObservation::ManifestlessNonempty,
        Theme::dark(false),
    );
    let local_uuid = format!("Local UUID: {LOCAL_WORKSPACE_ID}");

    for expected in [
        "Workspace identity",
        "Local workspace: family",
        &local_uuid,
        "Remote target: BRAIN:shared/brain",
        "Remote status: nonempty, no workspace manifest",
    ] {
        assert!(manifestless.contains(expected), "{manifestless}");
    }
    assert!(!manifestless.contains("Remote UUID:"), "{manifestless}");

    let matching = format_identity_summary(
        &local_name,
        local_workspace_id(),
        target,
        &crate::sync::identity::RemoteIdentityObservation::CompatibleManifest {
            workspace_id: local_workspace_id(),
        },
        Theme::dark(false),
    );
    assert!(
        matching.contains("Remote status: compatible workspace manifest"),
        "{matching}"
    );
    assert!(
        matching.contains(&format!("Remote UUID: {LOCAL_WORKSPACE_ID}")),
        "{matching}"
    );
}

#[test]
fn manifestless_adoption_prompts_only_when_exact_flag_authority_is_absent() {
    use std::cell::Cell;

    use crate::sync::identity::{ManifestlessRemoteAdoption, RemoteIdentityObservation};

    let prompts = Cell::new(0);
    let authorized = adoption_for_observation(
        local_workspace_id(),
        AdoptionAuthorization::Authorized,
        &RemoteIdentityObservation::ManifestlessNonempty,
        || -> Result<bool> { panic!("exact authority must not prompt") },
    )
    .unwrap();
    assert_eq!(
        authorized,
        ManifestlessRemoteAdoption::Authorized(local_workspace_id())
    );

    let interactive = adoption_for_observation(
        local_workspace_id(),
        AdoptionAuthorization::NeedsInteractiveConfirmation,
        &RemoteIdentityObservation::ManifestlessNonempty,
        || {
            prompts.set(prompts.get() + 1);
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(
        interactive,
        ManifestlessRemoteAdoption::Authorized(local_workspace_id())
    );
    assert_eq!(prompts.get(), 1);

    let refusal = adoption_for_observation(
        local_workspace_id(),
        AdoptionAuthorization::NeedsInteractiveConfirmation,
        &RemoteIdentityObservation::ManifestlessNonempty,
        || Ok(false),
    )
    .unwrap_err();
    assert!(refusal.to_string().contains("not confirmed"));

    let matching = adoption_for_observation(
        local_workspace_id(),
        AdoptionAuthorization::NeedsInteractiveConfirmation,
        &RemoteIdentityObservation::CompatibleManifest {
            workspace_id: local_workspace_id(),
        },
        || -> Result<bool> { panic!("matching identity must not prompt") },
    )
    .unwrap();
    assert_eq!(matching, ManifestlessRemoteAdoption::Refuse);
}
