#[test]
fn legacy_readiness_accepts_exactly_valid_user_ids() {
    enum Expected {
        Invalid,
        Incomplete,
        Ready,
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("family").unwrap();
    for (raw, expected) in [
        ("Pablo", Expected::Invalid),
        ("local_user", Expected::Invalid),
        (" pablo ", Expected::Invalid),
        ("", Expected::Incomplete),
        ("valid-kebab", Expected::Ready),
    ] {
        let record = WorkspaceRecord {
            workspace_id,
            root: temp.path().join("family"),
            aliases: BTreeSet::new(),
            local_user_id: raw.to_owned(),
            receiver_enabled: false,
            env: Map::new(),
        };
        let result = readiness_action_with_users(
            &name,
            &record,
            Ok(manifest.clone()),
            UsersStore::load_from(&temp.path().join(format!("missing-{raw:?}.json"))),
            InteractionMode::NonInteractive,
        );

        match expected {
            Expected::Invalid => {
                let error = result.unwrap_err();
                assert!(matches!(
                    error,
                    brain::workspace::ReadinessError::InvalidLegacyLocalUser { .. }
                ));
                assert!(
                    error
                        .to_string()
                        .contains("brain workspace repair -w family --local-user-id <USER_ID>")
                );
            }
            Expected::Incomplete => assert!(matches!(
                result,
                Err(brain::workspace::ReadinessError::Incomplete { .. })
            )),
            Expected::Ready => assert!(matches!(result, Ok(ReadinessAction::Ready(_)))),
        }
    }
}

fn users_named(ids: &[&str]) -> brain::users::Users {
    brain::users::Users {
        schema_version: brain::users::USERS_SCHEMA_VERSION,
        users: ids
            .iter()
            .map(|id| brain::users::User {
                id: brain::users::UserId::parse(id).unwrap(),
                name: (*id).to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            })
            .collect(),
    }
}

fn record_without_local_user(root: PathBuf, workspace_id: WorkspaceId) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id,
        root,
        aliases: BTreeSet::new(),
        local_user_id: String::new(),
        receiver_enabled: false,
        env: Map::new(),
    }
}

#[test]
fn sole_portable_user_is_auto_adopted_as_local_in_every_mode() {
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("brain").unwrap();
    let record = record_without_local_user(PathBuf::from("/brains/brain"), workspace_id);

    for mode in [
        InteractionMode::NonInteractive,
        InteractionMode::Interactive,
        InteractionMode::Internal,
    ] {
        let action = readiness_action_with_users(
            &name,
            &record,
            Ok(manifest.clone()),
            Ok(users_named(&["pablo"])),
            mode,
        )
        .unwrap();
        assert_eq!(
            action,
            ReadinessAction::AdoptLocalUser(brain::users::UserId::parse("pablo").unwrap()),
            "{mode:?}"
        );
    }
}
