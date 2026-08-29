#[test]
fn receiver_notice_cutover_errors_never_disclose_private_state_paths() {
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
            let down = fixture.run(&[
                "__migrate",
                "--from-version",
                env!("CARGO_PKG_VERSION"),
                "--to-version",
                "0.85.28",
            ]);
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
        let arguments = if direction == "upgrade" {
            [
                "__migrate",
                "--from-version",
                "0.85.28",
                "--to-version",
                env!("CARGO_PKG_VERSION"),
            ]
        } else {
            [
                "__migrate",
                "--from-version",
                env!("CARGO_PKG_VERSION"),
                "--to-version",
                "0.85.28",
            ]
        };

        let output = fixture.run(&arguments);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success());
        assert!(stderr.contains(&format!("{direction} receiver notice state")));
        assert!(
            !stderr.contains(state.to_string_lossy().as_ref()),
            "{direction} disclosed private receiver state path: {stderr}"
        );
    }
}
