use super::*;
use crate::{
    access::AccessMode,
    actor::ActorContext,
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceName},
};
use std::{os::unix::fs::PermissionsExt as _, sync::Arc};

fn prepared(root: &Path) -> LaunchRequest {
    if !root.join("AGENTS.md").exists() {
        std::fs::write(root.join("AGENTS.md"), "Shared workspace rules").unwrap();
    }
    if !crate::workspace::WorkspaceManifest::path(root).exists() {
        crate::workspace::WorkspaceManifest::new(
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        )
        .write_new(root)
        .unwrap();
    }
    let workspace = Arc::new(
        WorkspaceContext::new(
            root,
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            WorkspaceName::parse("family").unwrap(),
            root,
            "pablo",
            root,
        )
        .unwrap(),
    );
    let actor: ActorContext = serde_json::from_value(serde_json::json!({
        "user_id":"pablo", "display_name":"Pablo", "channel":"email"
    }))
    .unwrap();
    LaunchRequest::from_trusted_context(
        workspace,
        actor,
        SessionPlan::resume(AgentSession::new("resumed").unwrap()),
        None,
        AccessMode::Unrestricted,
    )
    .prepare_instructions()
    .unwrap()
}

fn executable(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn effective_native_and_inherited_prompts_are_preserved_exactly_once() {
    for inherited in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("configured-opencode");
        let effective =
            serde_json::json!({"agent":{"brain":{"prompt":"User rules é🙂"}}}).to_string();
        executable(
            &script,
            &format!(
                "[ \"$PWD\" = {} ] && [ \"$BRAIN_ACTOR_ID\" = pablo ] && [ \"$KEEP\" = 'literal value' ] || exit 64\n[ \"$*\" = '--model configured-model debug config --pure' ] || {{ touch unsafe-marker; exit 64; }}\nprintf '%s\\n' {}",
                shell_quote(&temp.path().canonicalize().unwrap().display().to_string()),
                shell_quote(&effective),
            ),
        );
        let prefix = format!(
            "KEEP='literal value' {} --model configured-model",
            shell_quote(&script.display().to_string())
        );
        let mut frontend = OpenCodeFrontend::for_workspace(&prefix, temp.path());
        if inherited {
            frontend.inherited_config = Some(effective);
        }
        let spec = frontend.launch_spec(&prepared(temp.path())).unwrap();
        assert!(spec.command.starts_with(&prefix));
        let config = &spec
            .environment
            .iter()
            .find(|(key, _)| key == "OPENCODE_CONFIG_CONTENT")
            .unwrap()
            .1;
        let value: serde_json::Value = serde_json::from_str(config).unwrap();
        let prompt = value["agent"]["brain"]["prompt"].as_str().unwrap();
        assert_eq!(prompt.matches("User rules é🙂").count(), 1, "{prompt}");
        assert!(prompt.contains("Shared workspace rules"));
        assert!(prompt.contains("Channel: email"));
        assert!(!temp.path().join("unsafe-marker").exists());
    }
}

#[test]
fn prefix_owned_config_is_refused_before_any_probe() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("configured-opencode");
    executable(
        &script,
        &format!(
            "touch {}; printf '{{}}'",
            shell_quote(&temp.path().join("probe-marker").display().to_string())
        ),
    );
    for prefix in [
        "OPENCODE_CONFIG_CONTENT='secret-config'",
        "env OPENCODE_CONFIG_CONTENT='secret-config'",
    ] {
        let frontend = OpenCodeFrontend::for_workspace(
            format!("{prefix} {}", shell_quote(&script.display().to_string())),
            temp.path(),
        );
        frontend
            .ensure_available()
            .expect_err("refuse before compatibility probe");
        assert!(!temp.path().join("probe-marker").exists());
        let error = frontend
            .launch_spec(&prepared(temp.path()))
            .expect_err("owned config assignment");
        assert!(
            error.to_string().contains("inherited environment"),
            "{error}"
        );
        assert!(!error.to_string().contains("secret-config"));
        assert!(!temp.path().join("probe-marker").exists());
    }
}

#[test]
fn malformed_unavailable_and_oversized_native_prompts_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("configured-opencode");
    let request = prepared(temp.path());
    for output in [
        "secret-invalid-json".to_owned(),
        "[]".to_owned(),
        r#"{"agent":[]}"#.to_owned(),
        r#"{"agent":{"brain":{"prompt":42}}}"#.to_owned(),
        serde_json::json!({"agent":{"brain":{"prompt":"x".repeat(20 * 1024)}}}).to_string(),
    ] {
        executable(&script, &format!("printf '%s\\n' {}", shell_quote(&output)));
        let frontend = OpenCodeFrontend::for_workspace(
            shell_quote(&script.display().to_string()),
            temp.path(),
        );
        let error = frontend
            .launch_spec(&request)
            .expect_err("invalid native prompt");
        assert!(!error.to_string().contains("secret-invalid-json"));
    }
    executable(&script, "exit 64");
    let frontend =
        OpenCodeFrontend::for_workspace(shell_quote(&script.display().to_string()), temp.path());
    assert!(frontend.launch_spec(&request).is_err());
}
