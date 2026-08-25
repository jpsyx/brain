use super::*;

use super::receiver_durable_support::{publish_valid_completion, publish_valid_rotated_completion};
use crate::server::receiver::{AttachmentRef, StagedAttachment};
use crate::state::ReceiverConversationIdentity;

use super::receiver_attachment_worker_support::ControlledAttachmentWorker;

#[derive(Clone, Copy, Debug)]
enum HistoryKind {
    Fresh,
    Resume,
}

fn long_staged_paths(app: &App) -> Vec<PathBuf> {
    let inbox = app.context.workspace().paths().inbox_dir();
    let mut parent = inbox;
    for depth in 0..3 {
        parent = parent.join(format!("{depth:02}-{}", "'$$$".repeat(55)));
    }
    std::fs::create_dir_all(&parent).expect("long receiver inbox path");

    (0..crate::server::receiver::MAX_ATTACHMENT_COUNT)
        .map(|index| {
            let path = parent.join(format!("attachment-{index:02}-é🙂.txt"));
            std::fs::write(&path, b"private attachment").expect("staged attachment");
            assert!(path.is_absolute());
            assert!(path.as_os_str().len() > 700);
            path
        })
        .collect()
}

fn prompt_from_command(kind: AgentKind, command: &str) -> String {
    let marker = match kind {
        AgentKind::OpenCode => " --prompt ",
        AgentKind::Claude | AgentKind::Codex => " -- ",
    };
    let quoted = command
        .rsplit_once(marker)
        .map(|(_, quoted)| quoted)
        .expect("frontend prompt argument");
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &format!("printf %s {quoted}")])
        .output()
        .expect("decode shell-quoted prompt");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("UTF-8 receiver prompt")
}

fn create_codex_rollout(temporary: &tempfile::TempDir, session_id: &str) -> PathBuf {
    let sessions = temporary.path().join("codex-sessions");
    let day = sessions.join("9999/12/31");
    std::fs::create_dir_all(&day).expect("Codex rollout day");
    std::fs::write(
        day.join(format!("rollout-9999-12-31T00-00-00-{session_id}.jsonl")),
        "{}\n",
    )
    .expect("Codex rollout");
    sessions
}

#[test]
fn localized_attachment_prompts_are_bounded_after_final_paths_for_every_frontend_and_history() {
    let quote_mix = "'$$$".repeat(crate::tui::receiver::planning::RECOVERY_PROMPT_BUDGET_BYTES);
    let message = format!("authenticated-current-start-é🙂-{quote_mix}-current-end");
    let transcript = format!("oldest-context\n{quote_mix}\nnewest-context-é🙂");

    for kind in AgentKind::ALL {
        for history in [HistoryKind::Fresh, HistoryKind::Resume] {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let cli = Cli::parse_from(["tasks"]);
            let mut app = test_app(&temporary, &cli, kind);
            app.receiver.record_intent(true);
            let paths = long_staged_paths(&app);
            let mut inbound = receiver_job(&app, sms_actor(), Channel::Sms, &message);
            inbound.attachments = paths
                .iter()
                .enumerate()
                .map(|(index, _)| AttachmentRef {
                    url: format!("https://media.example.test/private-{index:02}"),
                    provider_id: Some(format!("provider-{index:02}")),
                    content_type: Some("text/plain".to_owned()),
                    filename: Some(format!("attachment-{index:02}.txt")),
                })
                .collect();
            let identity = ReceiverConversationIdentity::sms(
                app.context.workspace().id(),
                inbound.actor.user_id().clone(),
            );
            let db = Db::open(app.context.workspace()).expect("state DB");
            let mut claude_transcript = None;
            let mut codex_sessions = None;
            let mut codex_override = None;
            if matches!(history, HistoryKind::Resume) {
                let mut seed = receiver_job(
                    &app,
                    sms_actor(),
                    Channel::Sms,
                    "establish native receiver history",
                );
                seed.provider_id = Some(format!("resume-seed-{}", kind.as_str()));
                seed.received_at_unix_ms = 50;
                let first = db
                    .accept_receiver_job(&seed, &identity)
                    .expect("accept seed receiver job");
                assert!(
                    db.update_receiver_conversation(
                        first.conversation_id(),
                        &transcript,
                        None,
                        51,
                    )
                    .expect("seed receiver transcript")
                );
                let first_transport = TransportRecording::default();
                app.brain
                    .replace_receiver_transport(first_transport.transport());
                app.tick_receiver();
                let native_id = match kind {
                    AgentKind::Claude => app
                        .receiver
                        .active_durable_run()
                        .expect("fresh Claude receiver")
                        .attribution
                        .registered_session()
                        .as_str()
                        .to_owned(),
                    AgentKind::Codex => "019feb9e-edc0-7252-945a-5e06a30e0eec".to_owned(),
                    AgentKind::OpenCode => "session-1".to_owned(),
                };
                match kind {
                    AgentKind::Claude => {
                        claude_transcript = Some(ClaudeTranscript::create(
                            app.context.workspace().root(),
                            &native_id,
                        ));
                        publish_valid_completion(&app, "seed response");
                    }
                    AgentKind::Codex | AgentKind::OpenCode => {
                        publish_valid_rotated_completion(&app, &native_id, "seed response");
                    }
                }
                app.tick_receiver();
                assert!(app.receiver.active_durable_run().is_none());
                if kind == AgentKind::Codex {
                    codex_sessions = Some(create_codex_rollout(&temporary, &native_id));
                    codex_override = codex_sessions.as_deref().map(|sessions| {
                        crate::agent::override_codex_sessions_dir_for_test(sessions)
                    });
                }
            }
            let accepted = db
                .accept_receiver_job(&inbound, &identity)
                .expect("accept durable receiver job");
            if matches!(history, HistoryKind::Fresh) {
                assert!(
                    db.update_receiver_conversation(
                        accepted.conversation_id(),
                        &transcript,
                        None,
                        101,
                    )
                    .expect("seed receiver conversation")
                );
            }
            let transport = TransportRecording::default();
            app.brain.replace_receiver_transport(transport.transport());
            let worker = ControlledAttachmentWorker::default();
            app.services
                .replace_receiver_attachment_runtime(Box::new(worker.clone()));

            app.tick_receiver();
            worker.complete(
                worker.stage(0),
                paths
                    .iter()
                    .map(|path| StagedAttachment {
                        source: "refreshed-provider-reference".to_owned(),
                        path: Some(path.clone()),
                        error: None,
                    })
                    .collect(),
            );
            app.tick_receiver();

            let specifications = transport.launch_specs();
            assert_eq!(specifications.len(), 1, "{} with {history:?}", kind.label());
            let prompt = prompt_from_command(kind, &specifications[0].command);
            assert!(
                prompt.len() <= crate::tui::receiver::planning::RECOVERY_PROMPT_BUDGET_BYTES,
                "{} with {history:?} raw prompt was {} bytes",
                kind.label(),
                prompt.len(),
            );
            assert!(prompt.starts_with("If the message asks to add, create, capture"));
            assert!(prompt.contains("authenticated-current-start-é🙂-"));
            assert!(prompt.contains("[Current authenticated message truncated]"));
            assert!(prompt.contains("Local attachment files:"));
            assert!(prompt.contains(&paths[0].display().to_string()));
            assert!(!prompt.contains("https://media.example.test/private-00"));
            match history {
                HistoryKind::Fresh => {
                    assert!(prompt.contains("[Earlier portable transcript omitted]"));
                    assert!(prompt.contains("newest-context-é🙂"));
                }
                HistoryKind::Resume => assert!(!prompt.contains("## Portable transcript")),
            }
            assert!(
                specifications[0].command.len()
                    <= crate::agent::frontend::SHELL_COMMAND_ARGUMENT_BUDGET_BYTES,
                "{} with {history:?} command was {} bytes",
                kind.label(),
                specifications[0].command.len(),
            );
            drop(codex_override);
            drop(codex_sessions);
            drop(claude_transcript);
        }
    }
}
