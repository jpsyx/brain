include!("test_support.rs");

#[test]
fn configured_claude_controller_rejects_a_version_without_prompt_id_hooks() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir().expect("temporary Claude command");
    let script = temporary.path().join("old-claude");
    std::fs::write(
        &script,
        "#!/bin/sh\n[ \"$1\" = --version ] || exit 64\nprintf '%s\\n' '2.1.195 (Claude Code)'\n",
    )
    .expect("write old Claude command");
    let mut permissions = std::fs::metadata(&script)
        .expect("old Claude metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).expect("make old Claude executable");
    let command = crate::agent::frontend::shell_quote(&script.display().to_string());
    let workspace = workspace();
    let actor = crate::actor::test_actor("pablo");
    let recording = Recording::default();
    let controller = AgentController::for_workspace_with_command(
        workspace,
        AgentKind::Claude,
        command,
        actor,
        Box::new(RecordingTransport {
            recording: recording.clone(),
            pending_text: None,
        }),
    );

    let error = controller
        .ensure_available()
        .expect_err("old Claude must fail controller preflight");

    assert_eq!(
        error.to_string(),
        "frontend error: Claude is incompatible: version 2.1.195 does not provide the required `prompt_id` hook field. Update Claude Code to 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command."
    );
    assert!(recording.events().is_empty());
}

#[test]
fn semantic_operations_deliver_follow_up_without_timer_ticks() {
    let (mut controller, recording, _, _) = controller();

    controller.type_text("hello").expect("type text");
    controller.submit_now().expect("submit");
    controller
        .queue_after_active_turn("next")
        .expect("queue after turn");

    assert_eq!(
        recording.events(),
        vec![
            Event::Type("hello".to_owned()),
            Event::Submit,
            Event::Queue("next".to_owned()),
        ]
    );
}

#[test]
fn repeated_follow_ups_preserve_fifo_order_before_new_session_and_shutdown() {
    let (mut controller, recording, _, _) = controller();

    controller.queue_after_active_turn("first").unwrap();
    controller.queue_after_active_turn("second").unwrap();
    controller.start_new_session().unwrap();
    controller.shutdown().unwrap();

    assert_eq!(
        recording.events(),
        vec![
            Event::Queue("first".to_owned()),
            Event::Queue("second".to_owned()),
            Event::FrontendNewSession,
            Event::TransportNewSession(InputSequence::bytes(NEW_SESSION_MARKER)),
            Event::Shutdown,
        ]
    );
}

#[test]
fn availability_is_checked_through_the_controller_facade() {
    let (controller, recording, _, _) = controller();

    controller.ensure_available().expect("frontend available");

    assert!(recording.events().is_empty());
}

#[test]
fn compatibility_failure_precedes_frontend_launch_and_transport_spawn() {
    let (mut controller, recording, workspace, actor) = controller();
    controller.frontend = Box::new(RecordingFrontend {
        recording: recording.clone(),
        available: false,
        command: "recording-agent".to_owned(),
    });
    let request = request(
        workspace,
        actor,
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );

    let error = controller.launch(&request).expect_err("preflight failure");

    assert_eq!(
        error,
        AgentError::Frontend("compatibility probe failed".to_owned())
    );
    assert!(recording.events().is_empty());
}

#[test]
fn failed_transport_spawn_rolls_back_frontend_launch_artifacts() {
    let (mut controller, recording, workspace, actor) = controller();
    controller.transport = Box::new(FailingSpawnTransport {
        recording: recording.clone(),
    });
    let request = request(
        workspace,
        actor,
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );

    let error = controller.launch(&request).expect_err("spawn must fail");

    assert_eq!(error, AgentError::Transport("spawn failed".to_owned()));
    assert_eq!(
        recording.events(),
        vec![
            Event::Launch(request.session_plan().clone()),
            Event::Spawn,
            Event::Rollback
        ]
    );
}

#[test]
fn shell_command_argument_over_budget_rolls_back_without_reaching_transport() {
    let (mut controller, recording, workspace, actor) = controller();
    controller.frontend = Box::new(RecordingFrontend {
        recording: recording.clone(),
        available: true,
        command: "x".repeat(96 * 1024 + 1),
    });
    let request = request(
        workspace,
        actor,
        SessionPlan::fresh(AgentSession::new("fresh-1").expect("session")),
    );

    let error = controller
        .launch(&request)
        .expect_err("oversized shell command must fail before spawn");

    assert_eq!(
        error,
        AgentError::Frontend(
            "agent launch command exceeds the 96 KiB shell argument safety limit".to_owned()
        )
    );
    assert_eq!(
        recording.events(),
        vec![
            Event::Launch(request.session_plan().clone()),
            Event::Rollback
        ]
    );
}

#[test]
fn configured_controller_selects_the_frontend_without_exposing_adapter_construction() {
    let workspace = workspace();
    let actor = crate::actor::test_actor("pablo");
    let command = crate::workspace::CommandContext::for_test(
        Arc::clone(&workspace),
        crate::workspace::RegistryStore::from_path(Path::new("/missing/env.json").to_path_buf()),
        "pablo",
    );
    let recording = Recording::default();

    let controller = AgentController::configured(
        &command,
        AgentKind::OpenCode,
        actor,
        Box::new(RecordingTransport {
            recording,
            pending_text: None,
        }),
    );

    assert_eq!(controller.kind(), AgentKind::OpenCode);
}

#[test]
fn launch_preserves_fresh_and_resume_session_selection() {
    let (mut controller, recording, workspace, actor) = controller();
    let fresh = SessionPlan::fresh(AgentSession::new("fresh-1").expect("session"));
    let resume = SessionPlan::resume(AgentSession::new("resume-1").expect("session"));

    controller
        .launch(&request(
            Arc::clone(&workspace),
            actor.clone(),
            fresh.clone(),
        ))
        .expect("fresh launch");
    controller
        .launch(&request(workspace, actor, resume.clone()))
        .expect("resume launch");

    assert_eq!(
        recording.events(),
        vec![
            Event::Launch(fresh),
            Event::Spawn,
            Event::Launch(resume),
            Event::Spawn,
        ]
    );
}

#[test]
fn workspace_only_launch_rejects_a_missing_capability_plan_before_frontend_or_transport() {
    let (mut controller, recording, workspace, actor) = controller();
    let request = trusted_request(workspace, actor, AccessMode::WorkspaceOnly);

    assert!(controller.launch(&request).is_err());
    assert!(recording.events().is_empty());
}

#[test]
fn workspace_only_launch_rejects_a_plan_for_another_access_mode() {
    let (mut controller, recording, workspace, actor) = controller();
    let plan = capabilities(workspace.id(), AccessMode::Unrestricted);
    let request = trusted_request(Arc::clone(&workspace), actor, AccessMode::WorkspaceOnly)
        .with_capability_plan(plan);

    assert!(controller.launch(&request).is_err());
    assert!(recording.events().is_empty());
}

#[test]
fn workspace_only_launch_rejects_a_plan_from_another_workspace_record() {
    let (mut controller, recording, workspace, actor) = controller();
    let foreign =
        WorkspaceId::parse("6fd873b7-f05a-4eb1-b92e-4b8ae3df8e11").expect("foreign workspace id");
    let plan = capabilities(foreign, AccessMode::WorkspaceOnly);
    let request =
        trusted_request(workspace, actor, AccessMode::WorkspaceOnly).with_capability_plan(plan);

    assert!(controller.launch(&request).is_err());
    assert!(recording.events().is_empty());
}

#[test]
fn unrestricted_launch_needs_no_capability_plan() {
    let (mut controller, recording, workspace, actor) = controller();
    let request = trusted_request(workspace, actor, AccessMode::Unrestricted);

    controller.launch(&request).expect("unrestricted launch");

    assert!(matches!(
        recording.events().as_slice(),
        [Event::Launch(_), Event::Spawn]
    ));
}

#[test]
fn access_context_accepts_only_unrestricted_without_a_plan_or_matching_workspace_only() {
    let cases = [
        (
            "unrestricted without plan",
            AccessMode::Unrestricted,
            None,
            true,
            true,
        ),
        (
            "unrestricted with matching unrestricted plan",
            AccessMode::Unrestricted,
            Some(AccessMode::Unrestricted),
            true,
            false,
        ),
        (
            "unrestricted with workspace-only plan",
            AccessMode::Unrestricted,
            Some(AccessMode::WorkspaceOnly),
            true,
            false,
        ),
        (
            "unrestricted with foreign unrestricted plan",
            AccessMode::Unrestricted,
            Some(AccessMode::Unrestricted),
            false,
            false,
        ),
        (
            "unrestricted with foreign workspace-only plan",
            AccessMode::Unrestricted,
            Some(AccessMode::WorkspaceOnly),
            false,
            false,
        ),
        (
            "workspace-only without plan",
            AccessMode::WorkspaceOnly,
            None,
            true,
            false,
        ),
        (
            "workspace-only with matching plan",
            AccessMode::WorkspaceOnly,
            Some(AccessMode::WorkspaceOnly),
            true,
            true,
        ),
        (
            "workspace-only with unrestricted plan",
            AccessMode::WorkspaceOnly,
            Some(AccessMode::Unrestricted),
            true,
            false,
        ),
        (
            "workspace-only with foreign plan",
            AccessMode::WorkspaceOnly,
            Some(AccessMode::WorkspaceOnly),
            false,
            false,
        ),
        (
            "workspace-only with foreign unrestricted plan",
            AccessMode::WorkspaceOnly,
            Some(AccessMode::Unrestricted),
            false,
            false,
        ),
    ];
    let foreign =
        WorkspaceId::parse("6fd873b7-f05a-4eb1-b92e-4b8ae3df8e11").expect("foreign workspace id");

    for (label, mode, plan_mode, matching_workspace, accepted) in cases {
        let (mut controller, recording, workspace, actor) = controller();
        let mut request = trusted_request(Arc::clone(&workspace), actor, mode);
        if let Some(plan_mode) = plan_mode {
            let source = if matching_workspace {
                workspace.id()
            } else {
                foreign
            };
            request = request.with_capability_plan(capabilities(source, plan_mode));
        }

        assert_eq!(controller.launch(&request).is_ok(), accepted, "{label}");
        assert_eq!(
            recording.events().is_empty(),
            !accepted,
            "{label} reached frontend or transport unexpectedly"
        );
    }
}

#[test]
fn completion_strategy_delegates_to_the_frontend() {
    let (controller, recording, _, _) = controller();

    assert_eq!(
        controller.completion_strategy(),
        Ok(CompletionStrategy::Hook)
    );
    assert!(recording.events().is_empty());
}

#[test]
fn shutdown_delegates_once_to_the_transport() {
    let (mut controller, recording, _, _) = controller();

    controller.shutdown().expect("shutdown");

    assert_eq!(recording.events(), vec![Event::Shutdown]);
}

#[test]
fn shutdown_stops_the_transport_even_when_frontend_availability_fails() {
    let (mut controller, recording, _, _) = controller();
    controller.frontend = Box::new(RecordingFrontend {
        recording: recording.clone(),
        available: false,
        command: "recording-agent".to_owned(),
    });

    let error = controller
        .shutdown()
        .expect_err("report unavailable frontend after shutdown");

    assert_eq!(recording.events(), vec![Event::Shutdown]);
    assert_eq!(
        error,
        AgentError::Frontend("compatibility probe failed".to_owned())
    );
}

#[test]
fn shutdown_reports_a_transport_kill_failure_and_remains_retryable() {
    let (mut controller, recording) = controller_with_shutdown_outcome(ShutdownOutcome::KillFailed);

    assert_eq!(
        controller.shutdown(),
        Err(AgentError::Transport(
            "PTY child termination failed".to_owned()
        ))
    );
    assert_eq!(
        controller.shutdown(),
        Err(AgentError::Transport(
            "PTY child termination failed".to_owned()
        ))
    );
    assert_eq!(recording.events(), vec![Event::Shutdown, Event::Shutdown]);
}

#[test]
fn shutdown_reports_a_child_that_remains_running_and_remains_retryable() {
    let (mut controller, recording) =
        controller_with_shutdown_outcome(ShutdownOutcome::StillRunning);

    assert_eq!(
        controller.shutdown(),
        Err(AgentError::Transport(
            "PTY child remained running after termination".to_owned()
        ))
    );
    assert_eq!(
        controller.shutdown(),
        Err(AgentError::Transport(
            "PTY child remained running after termination".to_owned()
        ))
    );
    assert_eq!(recording.events(), vec![Event::Shutdown, Event::Shutdown]);
}

#[test]
fn shutdown_is_idempotent_only_after_the_transport_confirms_exit() {
    let (mut controller, recording) =
        controller_with_shutdown_outcome(ShutdownOutcome::ConfirmedExit);

    controller.shutdown().expect("confirmed child exit");
    controller.shutdown().expect("already confirmed child exit");

    assert_eq!(recording.events(), vec![Event::Shutdown]);
}

#[test]
fn queueing_rejects_empty_text_before_calling_the_frontend_or_transport() {
    let (mut controller, recording, _, _) = controller();

    assert_eq!(
        controller.queue_after_active_turn("   "),
        Err(AgentError::EmptyInput)
    );

    assert!(recording.events().is_empty());
}

#[test]
fn starting_a_new_session_delegates_to_the_frontend() {
    let (mut controller, recording, _, _) = controller();

    controller.start_new_session().expect("new session");

    assert_eq!(
        recording.events(),
        vec![
            Event::FrontendNewSession,
            Event::TransportNewSession(InputSequence::bytes(NEW_SESSION_MARKER)),
        ]
    );
}
