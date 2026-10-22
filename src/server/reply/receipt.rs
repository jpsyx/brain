pub(in crate::server) fn queued_sms_response(syncing: bool) -> crate::server::http::Response {
    let text = if syncing {
        "Received. I am currently re-syncing all documents first before I can process requests, I will let you know once I can work on your message."
    } else {
        "Received. The message has been queued."
    };
    crate::server::http::Response::xml(
        200,
        format!("<Response><Message>{text}</Message></Response>"),
    )
}

pub(in crate::server) fn sms_receipt(
    workspace: std::sync::Arc<crate::workspace::WorkspaceContext>,
    registry_store: crate::workspace::RegistryStore,
    job: &crate::server::receiver::InboundJob,
) -> crate::server::http::Response {
    let command = crate::workspace::CommandContext {
        workspace,
        actor: job.actor.clone(),
        registry_store,
    };
    let configured = crate::sync::config::SyncConfig::load(&command).is_configured();
    let paths = command.workspace.paths();
    let running = configured && crate::sync::current::is_running(paths);
    let waiting_for_agent = configured
        && !running
        && crate::state::Db::receiver_waiting_for_agent_read_only(
            &paths.state_db(),
            command.workspace.id(),
            job.job_id,
        )
        .unwrap_or(true);
    let last_pull = if configured && !running && !waiting_for_agent {
        crate::sync::journal::Journal::downstream_completion_read_only(&paths.sync_journal())
            .unwrap_or_else(|_| {
                crate::logging::log("receiver receipt freshness unavailable");
                None
            })
    } else {
        None
    };
    queued_sms_response(waiting_for_sync(
        configured,
        running,
        waiting_for_agent,
        last_pull.as_deref(),
        chrono::Utc::now(),
    ))
}

fn waiting_for_sync(
    configured: bool,
    running: bool,
    waiting_for_agent: bool,
    last_pull: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    configured
        && (running
            || (!waiting_for_agent && crate::sync::freshness::message_pull_due(last_pull, now)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn receipt_explains_running_or_required_sync_but_not_disabled_or_fresh_sync() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-30T12:00:00Z")
            .unwrap()
            .to_utc();
        assert!(!super::waiting_for_sync(true, false, true, None, now));
        assert!(super::waiting_for_sync(true, true, true, None, now));
        for (configured, running, last_pull, expected) in [
            (false, false, None, false),
            (false, true, None, false),
            (true, true, Some("2026-07-30T12:00:00Z"), true),
            (true, false, None, true),
            (true, false, Some("2026-07-30T09:59:59Z"), true),
            (true, false, Some("2026-07-30T10:00:00Z"), false),
        ] {
            assert_eq!(
                super::waiting_for_sync(configured, running, false, last_pull, now),
                expected
            );
        }
    }
}
