use super::*;
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn codex_preserves_effective_developer_instructions_and_configured_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("codex-fixture");
    let output = serde_json::json!([{
        "type":"message", "role":"developer",
        "content":[{"type":"input_text","text":"Existing profile rules '\"é🙂"}],
        "internal_chat_message_metadata_passthrough":{
            "content_item_kinds":["generic.developer_instructions"],
            "content_item_metadata":[{"provenance":{"type":"developer_instructions","from_additional_requirements":false}}]
        }
    }]);
    std::fs::write(&script,format!(
        "#!/bin/sh\ncase \" $* \" in *\" mcp list --json \"*) printf '[]\\n'; exit 0;; esac\n[ \"$1\" = --profile ] && [ \"$2\" = chosen ] && [ \"$3\" = debug ] && [ \"$4\" = prompt-input ] || exit 64\nprintf '%s\\n' {}\n",shell_quote(&output.to_string())
    )).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let request = prepared(temp.path());
    let prefix = format!(
        "{} --profile chosen",
        shell_quote(&script.display().to_string())
    );
    let spec = CodexFrontend::new(&prefix).launch_spec(&request).unwrap();
    assert!(spec.command.starts_with(&prefix));
    assert!(
        spec.command.contains("Existing profile rules"),
        "{}",
        spec.command
    );
    assert!(spec.command.contains("Workspace guidance"));
    assert!(spec.command.contains(" resume 'resume-session'"));
}

fn prepared(root: &std::path::Path) -> LaunchRequest {
    let extension = root.join(".config/extensions/workspace.md");
    std::fs::create_dir_all(extension.parent().unwrap()).unwrap();
    std::fs::write(extension, "[workspace:context]\nWorkspace guidance\n").unwrap();
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
    LaunchRequest::from_trusted_context(
        workspace,
        actor(),
        SessionPlan::resume(AgentSession::new("resume-session").unwrap()),
        None,
        AccessMode::Unrestricted,
    )
    .prepare_instructions()
    .unwrap()
}

#[test]
fn codex_suppresses_quoted_dotted_mcps_and_integrations_only_for_the_probe() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("codex-suppression-fixture");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/fake_codex.sh");
    let expected = r#"mcp_servers={"foo.bar\"é"={enabled=false}}"#;
    std::fs::write(&script, format!(r#"#!/bin/sh
case " $* " in
  *" mcp list --json "*) printf '%s\n' '[{{"name":"foo.bar\"é","enabled":true}}]'; exit 0;;
esac
found=false
for arg do [ "$arg" = {} ] && found=true; done
if [ "$found" = false ]; then touch unsafe-marker; exit 64; fi
case " $* " in *" --disable apps --disable plugins --disable hooks --disable shell_snapshot "*) ;; *) touch unsafe-marker; exit 64;; esac
[ "$BRAIN_ACTOR_ID" = pablo ] || exit 64
exec {} "$@"
"#, shell_quote(expected),shell_quote(&fixture.display().to_string()))).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let spec = CodexFrontend::new(shell_quote(&script.display().to_string()))
        .launch_spec(&prepared(temp.path()))
        .unwrap();
    assert!(!temp.path().join("unsafe-marker").exists());
    assert!(!spec.command.contains("--disable apps"));
    assert!(!spec.command.contains("mcp_servers="));
    assert!(spec.command.contains("Workspace guidance"));
}

#[test]
fn codex_rejects_malformed_inventory_and_opaque_shell_commands() {
    let temp = tempfile::tempdir().unwrap();
    let request = prepared(temp.path());
    for command in [
        "printf secret-invalid-inventory",
        "sh -c 'printf fake'",
        "codex && printf fake",
    ] {
        let error = CodexFrontend::new(command)
            .launch_spec(&request)
            .expect_err("unsafe introspection");
        assert!(
            error
                .to_string()
                .contains("Existing instructions were not replaced")
        );
        assert!(!error.to_string().contains("secret-invalid-inventory"));
    }
}
