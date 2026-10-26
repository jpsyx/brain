use super::*;
use crate::{
    actor::Channel,
    agent::{AgentKind, LaunchSpec, OpenCodeFrontend, PiFrontend},
};

pub(super) fn arguments(spec: &LaunchSpec) -> Vec<String> {
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &spec.command])
        .current_dir(&spec.cwd)
        .output()
        .unwrap();
    assert!(output.status.success());
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
        .collect()
}

pub(super) fn delivered(kind: AgentKind, spec: &LaunchSpec, args: &[String]) -> String {
    match kind {
        AgentKind::Claude | AgentKind::Pi => args
            .windows(2)
            .rfind(|pair| pair[0] == "--append-system-prompt")
            .expect("explicit append instructions")[1]
            .clone(),
        AgentKind::Codex => {
            let value = args
                .iter()
                .find_map(|arg| arg.strip_prefix("developer_instructions="))
                .expect("explicit developer instructions");
            serde_json::from_str(value).unwrap()
        }
        AgentKind::OpenCode => {
            let config = spec
                .environment
                .iter()
                .find(|(name, _)| name == "OPENCODE_CONFIG_CONTENT")
                .unwrap();
            let value: serde_json::Value = serde_json::from_str(&config.1).unwrap();
            value["agent"]["brain"]["prompt"]
                .as_str()
                .unwrap()
                .to_owned()
        }
    }
}

#[test]
fn every_frontend_delivers_shared_instructions_for_every_channel_access_mode_and_session_plan() {
    for kind in AgentKind::ALL {
        let temp = tempfile::tempdir().unwrap();
        let guidance = "Channel: email\nUser guidance: quotes '\"$`\\ and Unicode é🙂";
        std::fs::write(temp.path().join("AGENTS.md"), guidance).unwrap();
        crate::workspace::WorkspaceManifest::new(
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        )
        .write_new(temp.path())
        .unwrap();
        let workspace = Arc::new(
            WorkspaceContext::new(
                temp.path(),
                WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
                WorkspaceName::parse("family").unwrap(),
                temp.path(),
                "member",
                temp.path(),
            )
            .unwrap(),
        );
        for channel in [Channel::Interactive, Channel::Sms, Channel::Email] {
            for mode in [AccessMode::Unrestricted, AccessMode::WorkspaceOnly] {
                for resumed in [false, true] {
                    let actor: ActorContext = serde_json::from_value(serde_json::json!({
                        "user_id":"member", "display_name":"Member", "channel":channel.as_str()
                    }))
                    .unwrap();
                    let session = AgentSession::new("instruction-session").unwrap();
                    let plan = if resumed {
                        SessionPlan::resume(session)
                    } else {
                        SessionPlan::fresh(session)
                    };
                    let request = LaunchRequest::from_trusted_context(
                        Arc::clone(&workspace),
                        actor,
                        plan,
                        Some("prompt '$`\\\" é🙂".to_owned()),
                        mode,
                    )
                    .with_hook_metadata(crate::agent::HookMetadata::new(vec![(
                        "BRAIN_RESPONSE_ID".to_owned(),
                        "response-identity".to_owned(),
                    )]))
                    .prepare_instructions()
                    .unwrap();
                    let command = "printf '%s\\0' configured-prefix";
                    let frontend: Box<dyn AgentFrontend> = match kind {
                        AgentKind::Claude => Box::new(ClaudeFrontend::new(
                            format!(
                                "{}/tests/fixtures/claude/fake_claude.sh configured-prefix",
                                env!("CARGO_MANIFEST_DIR")
                            ),
                            temp.path().into(),
                            temp.path().join("projects"),
                        )),
                        AgentKind::Codex => Box::new(CodexFrontend::new(format!(
                            "{}/tests/fixtures/codex/fake_codex.sh configured-prefix",
                            env!("CARGO_MANIFEST_DIR")
                        ))),
                        AgentKind::OpenCode => Box::new(OpenCodeFrontend::for_workspace(
                            format!(
                                "{}/tests/fixtures/opencode/capture_opencode.sh configured-prefix",
                                env!("CARGO_MANIFEST_DIR")
                            ),
                            temp.path(),
                        )),
                        AgentKind::Pi => Box::new(PiFrontend::for_workspace(
                            format!("{command} --append-system-prompt 'Existing pi rules'"),
                            temp.path(),
                        )),
                    };
                    let spec = frontend.launch_spec(&request).unwrap();
                    let args = arguments(&spec);
                    assert_eq!(args[0], "configured-prefix");
                    assert_eq!(args.last().unwrap(), "prompt '$`\\\" é🙂");
                    if kind == AgentKind::Pi {
                        assert!(args.windows(2).any(|pair| pair == ["--append-system-prompt", "Existing pi rules"]));
                    }
                    if kind == AgentKind::Claude {
                        assert!(
                            args.windows(2)
                                .any(|pair| pair == ["--system-prompt-snapshot", "off"])
                        );
                    }
                    let payload = delivered(kind, &spec, &args);
                    let line = payload
                        .lines()
                        .find(|line| line.starts_with('"'))
                        .expect("JSON workspace guidance");
                    let published = std::fs::read_to_string(temp.path().join("AGENTS.md")).unwrap();
                    assert!(published.starts_with(guidance));
                    assert_eq!(serde_json::from_str::<String>(line).unwrap(), published);
                    assert!(
                        payload.contains(&format!("Channel: {}", channel.as_str())),
                        "{kind:?}"
                    );
                    assert!(payload.contains(&format!("Access mode: {}", mode.as_config_value())));
                    assert_eq!(
                        payload.contains("Do not read, inspect, modify"),
                        mode == AccessMode::WorkspaceOnly
                    );
                    assert!(spec.environment.contains(&(
                        "BRAIN_RESPONSE_ID".to_owned(),
                        "response-identity".to_owned()
                    )));
                    assert_eq!(spec.cwd, temp.path());
                }
            }
        }
    }
}
