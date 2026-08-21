use super::*;

#[test]
fn receiver_secrets_are_known_but_redacted_from_env_output() {
    let mut map = Map::new();
    map.insert("twilio_auth_token".to_owned(), Value::from("twilio-secret"));
    map.insert(
        "resend_sending_api_key".to_owned(),
        Value::from("resend-secret"),
    );

    assert_eq!(
        resolve_one_at(std::path::Path::new(TEST_ROOT), &map, "twilio_auth_token"),
        Some("(set)".to_owned())
    );
    assert_eq!(
        resolve_one_at(
            std::path::Path::new(TEST_ROOT),
            &map,
            "resend_sending_api_key"
        ),
        Some("(set)".to_owned())
    );
}

#[test]
fn sync_transport_secrets_are_redacted_but_its_identifiers_still_show() {
    let map = serde_json::from_value(serde_json::json!({
        "sync": {
            "enabled": true,
            "b2_bucket": "pablo-brain",
            "b2_key_id": "0056682573a47420000000004",
            "b2_app_key": "b2-application-secret",
            "crypt_password": "obscured-pass",
            "crypt_password2": "obscured-salt"
        }
    }))
    .expect("env map");

    let rows = resolve_all_at(std::path::Path::new(TEST_ROOT), &map);
    let value_of = |name: &str| {
        rows.iter()
            .find(|row| row.name == name)
            .and_then(|row| row.value.clone())
    };

    assert_eq!(value_of("sync.b2_app_key").as_deref(), Some("(set)"));
    assert_eq!(value_of("sync.crypt_password").as_deref(), Some("(set)"));
    assert_eq!(value_of("sync.crypt_password2").as_deref(), Some("(set)"));
    // Identifiers are not credentials; they stay visible so a user can confirm
    // which bucket and key a workspace points at.
    assert_eq!(value_of("sync.b2_bucket").as_deref(), Some("pablo-brain"));
    assert_eq!(
        value_of("sync.b2_key_id").as_deref(),
        Some("0056682573a47420000000004")
    );
    let rendered = rows
        .iter()
        .filter_map(|row| row.value.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    for secret in ["b2-application-secret", "obscured-pass", "obscured-salt"] {
        assert!(!rendered.contains(secret), "{secret} leaked:\n{rendered}");
    }
}

#[test]
fn agent_capability_credentials_are_redacted_from_env_list_rows() {
    let map = serde_json::from_value(serde_json::json!({
        "agent_capabilities": {
            "mcps": [{
                "name": "notion",
                "url": "https://notion.example.test/mcp",
                "credentials": {
                    "bearer_token": "machine-secret",
                    "headers": {"Authorization": "header-secret"}
                }
            }]
        }
    }))
    .expect("env map");

    let rows = resolve_all_at(std::path::Path::new(TEST_ROOT), &map);

    assert!(rows.iter().any(|row| {
        row.name == "agent_capabilities.mcps.0.url"
            && row.value.as_deref() == Some("https://notion.example.test/mcp")
    }));
    assert!(rows.iter().any(|row| {
        row.name == "agent_capabilities.mcps.0.credentials.bearer_token"
            && row.value.as_deref() == Some("(set)")
    }));
    let rendered = rows
        .iter()
        .filter_map(|row| row.value.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("machine-secret"));
    assert!(!rendered.contains("header-secret"));
}

#[test]
fn set_path_addresses_one_element_of_an_env_array() {
    // `skill_sessions` is an array of objects, so amending one session's prompt
    // must be a dotted write like any nested object field — and must leave its
    // siblings, and the entry's other fields, alone.
    let mut map: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "skill_sessions": [
            {"title": "Email triage", "prompt": "/email-triage"},
            {"title": "Weekly review", "prompt": "/triage weekly"},
        ]
    }))
    .unwrap();

    set_path(
        &mut map,
        "skill_sessions.0.prompt",
        Value::from("/email-triage --fast"),
    )
    .unwrap();

    assert_eq!(
        map["skill_sessions"][0]["prompt"],
        Value::from("/email-triage --fast")
    );
    assert_eq!(
        map["skill_sessions"][0]["title"],
        Value::from("Email triage")
    );
    assert_eq!(
        map["skill_sessions"][1]["prompt"],
        Value::from("/triage weekly")
    );
}

#[test]
fn set_path_refuses_an_array_index_that_does_not_exist() {
    // Growing a list by writing past its end would silently invent an entry with
    // no prompt, so an out-of-range index is an error the user can read.
    let mut map: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "skill_sessions": [{"prompt": "/email-triage"}]
    }))
    .unwrap();

    let error = set_path(&mut map, "skill_sessions.4.prompt", Value::from("/x"))
        .expect_err("out-of-range index");

    assert!(error.to_string().contains("skill_sessions.4"), "{error}");
    assert_eq!(map["skill_sessions"].as_array().map(Vec::len), Some(1));
}

/// Retrieving inbound mail and sending replies need different Resend
/// permissions, and a full-access key used for sending fans every outbound
/// event out to every webhook on the account. Two keys is therefore the only
/// working shape, and each variable's help text has to say which scope it
/// wants — the whole failure is invisible until an email silently stops
/// arriving.
#[test]
fn each_resend_key_documents_the_scope_it_needs() {
    let receiving = crate::env::schema::VARS
        .iter()
        .find(|spec| spec.name == "resend_full_access_api_key")
        .expect("receiving key is declared");
    let sending = crate::env::schema::VARS
        .iter()
        .find(|spec| spec.name == "resend_sending_api_key")
        .expect("sending key is declared");

    assert!(
        receiving.description.to_lowercase().contains("full access"),
        "receiving key must ask for full access: {}",
        receiving.description
    );
    assert!(
        sending.description.to_lowercase().contains("sending"),
        "sending key must ask for a sending-only scope: {}",
        sending.description
    );
    assert!(
        !crate::env::schema::VARS
            .iter()
            .any(|spec| spec.name == "resend_api_key"),
        "the single combined key must be gone, not quietly accepted alongside the two"
    );
}
