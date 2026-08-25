use std::path::PathBuf;

use crate::{agent::AgentKind, server::receiver::AttachmentRef};

use super::{
    BindingKind, CURRENT_PROMPT, RECOVERY_PROMPT_BUDGET_BYTES, build_receiver_launch_plan,
    durable_fixture, durable_fixture_with_input, fresh_session, render_receiver_launch_with_paths,
};

#[test]
fn planner_rejects_incomplete_local_attachment_input() {
    let (job, conversation) =
        durable_fixture(AgentKind::Claude, BindingKind::Absent, "portable context");

    let plan = build_receiver_launch_plan(&job, &conversation, &[], fresh_session(), None);

    assert!(plan.is_none());
}

#[test]
fn recovery_prompt_keeps_whole_local_paths_and_marks_omissions() {
    let attachments = (0..10)
        .map(|index| AttachmentRef {
            url: format!("https://attachments.example.test/oversized-{index}"),
            provider_id: Some(format!("media-{index}")),
            content_type: Some("image/png".to_owned()),
            filename: Some(format!("attachment-{index}.png")),
        })
        .collect::<Vec<_>>();
    let paths = (0..attachments.len())
        .map(|index| {
            PathBuf::from(format!(
                "/workspaces/family/{}/attachment-{index:02}-é🙂.bin",
                "local-'$$$-".repeat(1_000)
            ))
        })
        .collect::<Vec<_>>();
    let first_line = format!(
        "- path={}",
        serde_json::to_string(&paths[0].display().to_string()).unwrap()
    );
    let last_line = format!(
        "- path={}",
        serde_json::to_string(&paths[9].display().to_string()).unwrap()
    );

    for kind in AgentKind::ALL {
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Absent,
            "portable context",
            CURRENT_PROMPT,
            attachments.clone(),
        );
        let plan =
            render_receiver_launch_with_paths(&job, &conversation, &paths, fresh_session(), None);
        let prompt = plan.initial_prompt();

        assert!(
            prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
        assert!(prompt.contains("\n\nLocal attachment files:\n"));
        assert!(prompt.contains(&first_line));
        assert!(prompt.contains("[Additional local attachment files omitted]"));
        assert!(!prompt.contains(&last_line));
        for line in prompt.lines().filter(|line| line.starts_with("- path=")) {
            serde_json::from_str::<String>(&line[7..]).expect("complete JSON path record");
        }
    }
}
