#[test]
fn receiver_migration_errors_never_disclose_private_state_paths() {
    for direction in ["upgrade", "downgrade"] {
        let fixture = Fixture::new();
        fixture.seed_pre_receiver_state();
        let initial = fixture.run(&["server", "status"]);
        assert!(
            initial.status.success(),
            "{}",
            String::from_utf8_lossy(&initial.stderr)
        );
        let state = fixture.state_db("11111111-1111-4111-8111-111111111111");
        if direction == "upgrade" {
            let down = fixture.acknowledged_down("0.85.28");
            assert!(
                down.status.success(),
                "{}",
                String::from_utf8_lossy(&down.stderr)
            );
        }
        rusqlite::Connection::open(&state)
            .expect("receiver state")
            .execute_batch(
                "DROP TABLE receiver_jobs;
                 CREATE TABLE receiver_jobs (damaged TEXT);",
            )
            .expect("stage cutover failure");
        let output = if direction == "upgrade" {
            fixture.run(&[
                "__migrate",
                "--from-version",
                "0.85.28",
                "--to-version",
                env!("CARGO_PKG_VERSION"),
            ])
        } else {
            fixture.acknowledged_down("0.85.28")
        };
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success());
        if direction == "upgrade" {
            assert!(stderr.contains("upgrade receiver notice state"), "{stderr}");
        } else {
            // Receipt invalidation now opens damaged state before this older
            // downgrade. Exact notice-wrapper coverage lives beside that seam.
            assert!(stderr.contains("no such column"), "{stderr}");
        }
        assert!(
            !stderr.contains(state.to_string_lossy().as_ref()),
            "{direction} disclosed private receiver state path: {stderr}"
        );
    }
}
