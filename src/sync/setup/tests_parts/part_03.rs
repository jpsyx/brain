
#[test]
fn setup_refuses_an_incomplete_migration_before_remote_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());
    let journal = paths.migration_journal();
    std::fs::create_dir_all(journal.parent().unwrap()).unwrap();
    std::fs::write(journal, b"active migration\n").unwrap();
    let identity_entered = std::cell::Cell::new(false);

    let error = run_setup_stages(
        &paths,
        || {
            identity_entered.set(true);
            Ok(())
        },
        || Ok(()),
        || Ok(crate::sync::verify::Outcome::Clean),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("workspace migration is incomplete")
    );
    assert!(!identity_entered.get());
}

#[test]
fn identity_refusal_preserves_credentials_and_skips_the_baseline() {
    let stages = RefCell::new(Vec::new());
    let temporary = tempfile::tempdir().unwrap();
    let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());

    let error = run_setup_stages(
        &paths,
        || {
            stages.borrow_mut().push("identity");
            anyhow::bail!("wrong workspace")
        },
        || {
            stages.borrow_mut().push("credentials");
            Ok(())
        },
        || {
            stages.borrow_mut().push("baseline");
            Ok(crate::sync::verify::Outcome::Clean)
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("wrong workspace"));
    assert_eq!(*stages.borrow(), ["identity"]);
}

#[test]
fn setup_persists_credentials_only_after_a_clean_baseline() {
    let stages = RefCell::new(Vec::new());
    let temporary = tempfile::tempdir().unwrap();
    let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());

    let error = run_setup_stages(
        &paths,
        || {
            stages.borrow_mut().push("identity");
            Ok(())
        },
        || {
            stages.borrow_mut().push("credentials");
            Ok(())
        },
        || {
            stages.borrow_mut().push("baseline");
            Ok(crate::sync::verify::Outcome::Aborted(
                "max-delete guard".to_owned(),
            ))
        },
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("baseline was not clean"),
        "{error:#}"
    );
    assert_eq!(*stages.borrow(), ["identity", "baseline"]);
}

#[test]
fn setup_preserves_unsaved_credentials_on_attention_or_baseline_error() {
    let cases = [
        Ok(crate::sync::verify::Outcome::NeedsAttention(
            "conflict copies".to_owned(),
        )),
        Err(anyhow::anyhow!("transport failed")),
    ];
    for baseline_result in cases {
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());
        let credentials_persisted = std::cell::Cell::new(false);

        let error = run_setup_stages(
            &paths,
            || Ok(()),
            || {
                credentials_persisted.set(true);
                Ok(())
            },
            || baseline_result,
        )
        .unwrap_err();

        assert!(!credentials_persisted.get(), "{error:#}");
    }
}
