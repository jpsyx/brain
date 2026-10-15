use super::*;
#[path = "../support/task_removal_canonical.rs"]
mod fixture;

#[test]
fn canonical_middle_removal_cli_rewires_archived_project_and_agenda() {
    let f = Fixture::new();
    let date = chrono::Local::now().date_naive().to_string();
    let agenda = f.temp.path().join("agendas").join(format!("{date}.md"));
    fixture::seed(&f.root, &agenda);
    let before = fixture::images(&f.root, &agenda);
    let preview = document(
        &f.run(&[
            "tasks",
            "remove",
            "T2",
            "--dependents",
            "rewire",
            "--dry-run",
            "--json",
        ]),
        0,
    );
    assert_eq!(preview["ready"], true);
    assert_eq!(fixture::images(&f.root, &agenda), before);
    let receipt = document(
        &f.run(&[
            "tasks",
            "remove",
            "T2",
            "--dependents",
            "rewire",
            "--expect-plan",
            preview["fingerprint"].as_str().unwrap(),
            "--yes",
            "--json",
        ]),
        0,
    );
    assert_eq!(receipt["committed"], true);
    assert_eq!(receipt["removed"][0]["task_id"], "T2");
    fixture::assert_after(&f.root, &agenda, &date);
}

#[test]
fn consent_preview_names_blocked_edges_and_surviving_rewrites() {
    let f = Fixture::new();
    let agenda = f.temp.path().join("agenda.md");
    fixture::seed(&f.root, &agenda);
    let before = fixture::images(&f.root, &agenda);
    let blocked = document(&f.run(&["tasks", "remove", "T2", "--dry-run", "--json"]), 2);
    assert_eq!(blocked["blockers"][0]["details"]["task_name"], "Last");
    assert_eq!(
        blocked["blockers"][0]["details"]["prerequisite_name"],
        "Middle"
    );
    let output = f.run(&["tasks", "remove", "T2", "--dry-run"]);
    let text = String::from_utf8_lossy(&output.stderr);
    for name in ["T3", "Last", "T2", "Middle"] {
        assert!(text.contains(name), "{text}");
    }
    for (policy, expected) in [
        ("rewire", json!([{"task_id":"T1","task_name":"First"}])),
        ("unlink", json!([])),
    ] {
        let value = document(
            &f.run(&[
                "tasks",
                "remove",
                "T2",
                "--dependents",
                policy,
                "--dry-run",
                "--json",
            ]),
            0,
        );
        let effects = &value["effects"]["relationships"];
        assert_eq!(
            effects["tasks"][0]["task"],
            json!({"task_id":"T3","task_name":"Last"})
        );
        assert_eq!(
            effects["tasks"][0]["previous"],
            json!([{"task_id":"T2","task_name":"Middle"}])
        );
        assert_eq!(effects["tasks"][0]["next"], expected);
        assert!(
            effects["projects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["path"] == fixture::METADATA)
        );
        let output = f.run(&["tasks", "remove", "T2", "--dependents", policy, "--dry-run"]);
        assert!(output.stdout.is_empty());
        let text = String::from_utf8_lossy(&output.stderr);
        for name in ["Last", "Middle", "Project tasks"] {
            assert!(text.contains(name), "{text}");
        }
    }
    assert_eq!(fixture::images(&f.root, &agenda), before);
}

#[test]
fn consent_cross_kind_prerequisite_keeps_its_captured_name() {
    let f = Fixture::new();
    f.seed("T1,First,not_started,H2,,2026-10-14,\nT2,Last,not_started,T1,,2026-10-14,\n");
    std::fs::write(
        f.root.join("tasks/habits.csv"),
        "task_id,task_name,status\nH2,Walk,not_started\n",
    )
    .unwrap();
    let metadata_path = f.root.join("projects/sample__outcome/.METADATA.json");
    let mut metadata: Value =
        serde_json::from_slice(&std::fs::read(&metadata_path).unwrap()).unwrap();
    metadata["tasks"] = json!(["T1", "T2", "H2"]);
    std::fs::write(metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    let before = f.bytes();
    let value = document(
        &f.run(&[
            "tasks",
            "remove",
            "T1",
            "--dependents",
            "rewire",
            "--dry-run",
            "--json",
        ]),
        0,
    );
    assert_eq!(
        value["effects"]["relationships"]["tasks"][0]["next"][0],
        json!({"task_id":"H2","task_name":"Walk"})
    );
    assert_eq!(
        value["effects"]["relationships"]["projects"][0]["next"][1],
        json!({"task_id":"H2","task_name":"Walk"})
    );
    assert_eq!(f.bytes(), before);
}
