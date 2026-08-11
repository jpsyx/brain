use super::support::*;

#[test]
fn two_live_workspace_routes_render_only_their_own_habits() {
    let server = ServerFixture::new(FAMILY_ID);

    let family = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.family_lease, server.family_ingress
    ));
    let personal = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.personal_lease, server.personal_ingress
    ));

    assert!(family.starts_with("HTTP/1.1 200"), "{family}");
    assert!(family.contains("Family habit"), "{family}");
    assert!(!family.contains("Personal habit"), "{family}");
    assert!(
        family.contains(&format!(
            "/local/{}/w/{}/habits/done",
            server.family_lease, server.family_ingress
        )),
        "the rendered page must preserve its opaque ingress in completion requests"
    );
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
    assert!(personal.contains("Personal habit"), "{personal}");
    assert!(!personal.contains("Family habit"), "{personal}");
    assert!(
        personal.contains(&format!(
            "/local/{}/w/{}/habits/done",
            server.personal_lease, server.personal_ingress
        )),
        "the rendered personal page must preserve only its ingress"
    );
}

#[test]
fn habits_post_mutates_only_the_workspace_named_by_ingress() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);

    let response = server.post(
        &format!(
            "/local/{}/w/{}/habits/done",
            server.family_lease, server.family_ingress
        ),
        r#"{"task_id":"H1"}"#,
    );

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
    let family = String::from_utf8(habits_bytes(&server.family_root)).expect("family CSV utf8");
    assert!(family.contains("H1,Family habit,done"), "{family}");
}

#[test]
fn skill_session_completion_is_recorded_only_for_the_ingress_workspace() {
    let server = ServerFixture::new(FAMILY_ID);
    let signal_file = |id: &str| {
        brain::workspace::WorkspacePaths::new(server.home.path(), workspace_id(id))
            .cache_dir()
            .join("skill-sessions")
            .join("family-triage.json")
    };

    let response = server.post(
        &format!(
            "/local/{}/w/{}/session/done",
            server.family_lease, server.family_ingress
        ),
        r#"{"token":"family-triage"}"#,
    );

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(
        brain::skill_session::signal::read_signal(
            &workspace(
                server.home.path(),
                "family",
                FAMILY_ID,
                &server.family_root,
            ),
            "family-triage"
        )
        .expect("family completion signal")
        .token,
        "family-triage"
    );
    assert!(signal_file(FAMILY_ID).is_file());
    assert!(!signal_file(PERSONAL_ID).exists());
}

#[test]
fn global_and_unknown_ingress_routes_never_fall_back_to_default() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);

    for path in ["/habits".to_owned(), format!("/w/{UNKNOWN_ID}/habits")] {
        let get = server.get(&path);
        let post_path = format!("{path}/done");
        let post = server.post(&post_path, r#"{"task_id":"H1"}"#);
        assert!(!get.starts_with("HTTP/1.1 200"), "{path}: {get}");
        assert!(!post.starts_with("HTTP/1.1 200"), "{post_path}: {post}");
        assert!(!get.contains("Personal habit"), "{path}: {get}");
    }
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
}

#[test]
fn habits_requests_reject_a_manifest_identity_mismatch() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);
    let family_before = habits_bytes(&server.family_root);
    std::fs::write(
        server.family_root.join(".config/workspace.json"),
        format!(
            "{{\"schema_version\":1,\"workspace_id\":\"{UNKNOWN_ID}\",\"receiver_ingress_id\":\"{}\",\"minimum_brain_version\":\"0.27.2\"}}\n",
            server.family_ingress
        ),
    )
    .expect("replace family manifest identity");

    let get = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.family_lease, server.family_ingress
    ));
    let post = server.post(
        &format!(
            "/local/{}/w/{}/habits/done",
            server.family_lease, server.family_ingress
        ),
        r#"{"task_id":"H1"}"#,
    );

    assert!(!get.starts_with("HTTP/1.1 200"), "{get}");
    assert!(!post.starts_with("HTTP/1.1 200"), "{post}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
    assert_eq!(habits_bytes(&server.family_root), family_before);
}

#[test]
fn habits_requests_reject_an_unavailable_selected_root() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);
    std::fs::remove_dir_all(&server.family_root).expect("remove temporary family root");

    let get = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.family_lease, server.family_ingress
    ));
    let post = server.post(
        &format!(
            "/local/{}/w/{}/habits/done",
            server.family_lease, server.family_ingress
        ),
        r#"{"task_id":"H1"}"#,
    );

    assert!(!get.starts_with("HTTP/1.1 200"), "{get}");
    assert!(!post.starts_with("HTTP/1.1 200"), "{post}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
}

#[test]
fn known_ingress_without_its_live_tui_is_unavailable_while_peer_stays_routable() {
    let server = ServerFixture::new(FAMILY_ID);
    server
        .client
        .unregister_generation(server.generation, server.family_lease)
        .expect("unregister family TUI");

    let family = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.family_lease, server.family_ingress
    ));
    let personal = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.personal_lease, server.personal_ingress
    ));

    assert!(family.starts_with("HTTP/1.1 503"), "{family}");
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
    assert!(personal.contains("Personal habit"), "{personal}");
}

#[test]
fn local_actions_reject_a_peer_workspace_lease_capability() {
    let server = ServerFixture::new(FAMILY_ID);
    let family_before = habits_bytes(&server.family_root);

    let page = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.personal_lease, server.family_ingress
    ));
    let mutation = server.post(
        &format!(
            "/local/{}/w/{}/habits/done",
            server.personal_lease, server.family_ingress
        ),
        r#"{"task_id":"H1"}"#,
    );

    assert!(page.starts_with("HTTP/1.1 404"), "{page}");
    assert!(mutation.starts_with("HTTP/1.1 404"), "{mutation}");
    assert_eq!(habits_bytes(&server.family_root), family_before);
}

#[test]
fn receiver_disabled_live_ingress_still_allows_local_habits_actions() {
    let server = ServerFixture::new(FAMILY_ID);
    server.disable_family_receiver();

    let family = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.family_lease, server.family_ingress
    ));
    let family_session = server.post(
        &format!(
            "/local/{}/w/{}/session/done",
            server.family_lease, server.family_ingress
        ),
        r#"{"token":"must-not-land"}"#,
    );
    let personal = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.personal_lease, server.personal_ingress
    ));

    assert!(family.starts_with("HTTP/1.1 200"), "{family}");
    assert!(
        family_session.starts_with("HTTP/1.1 200"),
        "{family_session}"
    );
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
    assert!(personal.contains("Personal habit"), "{personal}");
}

#[test]
fn persisted_disable_does_not_block_local_habits_routes() {
    let server = ServerFixture::new(FAMILY_ID);
    server.persist_family_receiver_disabled();

    let family = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.family_lease, server.family_ingress
    ));
    let personal = server.get(&format!(
        "/local/{}/w/{}/habits",
        server.personal_lease, server.personal_ingress
    ));

    assert!(family.starts_with("HTTP/1.1 200"), "{family}");
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
}
