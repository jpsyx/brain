/// Brain only learned about `capture/` at 0.94.0, so every workspace
/// registered before then is missing it. An upgrade provisions **every**
/// registered workspace at once rather than waiting for the user to select
/// each one in turn.
#[test]
fn an_upgrade_gives_every_registered_workspace_the_capture_in_basket() {
    let fixture = Fixture::new();
    assert!(!fixture.family.join("capture").exists());
    assert!(!fixture.work.join("capture").exists());

    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        "0.93.0",
        "--to-version",
        env!("CARGO_PKG_VERSION"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.family.join("capture").is_dir());
    assert!(fixture.work.join("capture").is_dir());
}

/// The in-basket is where the user's own material lives, so a rollback must
/// not trade their notes for a tidy directory listing. An empty one is
/// removed; one with anything in it stays exactly where it is.
#[test]
fn a_downgrade_removes_an_empty_in_basket_and_keeps_a_filled_one() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .run(&[
                "__migrate",
                "--from-version",
                "0.93.0",
                "--to-version",
                env!("CARGO_PKG_VERSION"),
            ])
            .status
            .success()
    );
    std::fs::write(fixture.work.join("capture/receipt.png"), b"png").expect("captured file");

    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.93.0",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !fixture.family.join("capture").exists(),
        "an empty in-basket should not survive a downgrade"
    );
    assert!(
        fixture.work.join("capture/receipt.png").is_file(),
        "captured material must survive a downgrade"
    );
}

/// Re-running the current version reconciles rather than skips, so an
/// in-basket a tool pruned (or the user deleted) comes back without the user
/// having to notice it was gone.
#[test]
fn reconciling_the_current_version_restores_a_deleted_in_basket() {
    let fixture = Fixture::new();
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        fixture
            .run(&[
                "__migrate",
                "--from-version",
                "0.93.0",
                "--to-version",
                version
            ])
            .status
            .success()
    );
    std::fs::remove_dir(fixture.family.join("capture")).expect("remove in-basket");

    assert!(
        fixture
            .run(&[
                "__migrate",
                "--from-version",
                version,
                "--to-version",
                version
            ])
            .status
            .success()
    );

    assert!(fixture.family.join("capture").is_dir());
}
