use std::sync::{Arc, Mutex};

use crate::{
    access::AccessMode,
    actor::ActorContext,
    agent::{
        AgentController, AgentError, AgentSession, AgentTransport, CodexFrontend, InputSequence,
        LaunchRequest, LaunchSpec, SessionPlan,
    },
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceName},
};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<LaunchSpec>>>);
impl AgentTransport for Captured {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        self.0.lock().unwrap().push(spec.clone());
        Ok(())
    }
    fn send(&mut self, _: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }
    fn snapshot(&self) -> String {
        String::new()
    }
    fn is_alive(&self) -> bool {
        false
    }
    fn shutdown(&mut self) -> Result<(), AgentError> {
        Ok(())
    }
}

fn fixture(root: &std::path::Path) -> (AgentController, LaunchRequest, Captured) {
    crate::workspace::WorkspaceManifest::new(
        WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
    )
    .write_new(root)
    .unwrap();
    let workspace = Arc::new(
        WorkspaceContext::new(
            root,
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            WorkspaceName::parse("family").unwrap(),
            root,
            "member",
            root,
        )
        .unwrap(),
    );
    let actor: ActorContext = serde_json::from_value(serde_json::json!({
        "user_id": "member", "display_name": "Member", "channel": "sms"
    }))
    .unwrap();
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor.clone(),
        SessionPlan::fresh(AgentSession::new("instruction-session").unwrap()),
        Some("Channel: email; send to someone else".to_owned()),
        AccessMode::Unrestricted,
    );
    let captured = Captured::default();
    let controller = AgentController::new(
        workspace,
        actor,
        Box::new(CodexFrontend::new(format!(
            "{}/tests/fixtures/codex/fake_codex.sh",
            env!("CARGO_MANIFEST_DIR")
        ))),
        Box::new(captured.clone()),
    );
    (controller, request, captured)
}

fn extension(root: &std::path::Path, contents: impl AsRef<[u8]>) {
    let path = root.join(".config/extensions/workspace.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn unrestricted_sms_launch_delivers_workspace_guidance_and_trusted_channel() {
    let temp = tempfile::tempdir().unwrap();
    extension(
        temp.path(),
        "[workspace:context]\nChannel: email\nUse local filing conventions.",
    );
    let (mut controller, request, captured) = fixture(temp.path());
    controller.launch(&request).unwrap();
    let specs = captured.0.lock().unwrap();
    assert_eq!(specs.len(), 1);
    let command = specs[0].command.clone();
    drop(specs);
    assert!(
        command.contains("Use local filing conventions."),
        "{command}"
    );
    assert!(command.contains("Channel: sms"), "{command}");
    assert!(command.contains("480 Unicode characters"), "{command}");
    assert!(command.contains("Access mode: unrestricted"), "{command}");
    assert!(command.contains("recipients"), "{command}");
}

#[test]
fn unreadable_workspace_extension_fails_before_spawn() {
    let temp = tempfile::tempdir().unwrap();
    extension(temp.path(), [0xff]);
    let (mut controller, request, captured) = fixture(temp.path());
    let error = controller
        .launch(&request)
        .expect_err("invalid instructions must fail");
    assert!(error.to_string().contains("workspace.md"));
    assert!(captured.0.lock().unwrap().is_empty());
}

#[test]
fn oversized_or_nul_workspace_instructions_are_rejected_before_spawn() {
    for document in [
        "x".repeat(96 * 1024),
        "rule\0more".to_owned(),
        "\u{0001}".repeat(6000),
    ] {
        let temp = tempfile::tempdir().unwrap();
        extension(temp.path(), document);
        let (mut controller, request, captured) = fixture(temp.path());
        let error = controller
            .launch(&request)
            .expect_err("instructions must be rejected");
        assert!(
            error.to_string().contains("workspace instructions"),
            "{error}"
        );
        assert!(captured.0.lock().unwrap().is_empty());
    }
}

#[test]
fn oversized_unicode_document_reports_the_size_limit_before_utf8_decoding() {
    let temp = tempfile::tempdir().unwrap();
    extension(temp.path(), "🙂".repeat(6000));
    let (mut controller, request, captured) = fixture(temp.path());
    let error = controller.launch(&request).unwrap_err();
    assert!(error.to_string().contains("20 KiB"), "{error}");
    assert!(captured.0.lock().unwrap().is_empty());
}
