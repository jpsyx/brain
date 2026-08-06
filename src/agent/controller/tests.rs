include!("test_support.rs");

#[test]
fn semantic_operations_are_forwarded_without_callers_constructing_keystrokes() {
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
            Event::Type("next".to_owned()),
        ]
    );

    controller.tick().expect("first delayed-input tick");
    controller.tick().expect("second delayed-input tick");

    assert_eq!(
        recording.events(),
        vec![
            Event::Type("hello".to_owned()),
            Event::Submit,
            Event::Type("next".to_owned()),
            Event::Queue("next".to_owned()),
        ]
    );
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
fn completion_strategy_and_transcript_lookup_delegate_to_the_frontend() {
    let (controller, recording, _, _) = controller();
    let session = AgentSession::new("session-1").expect("session");

    assert_eq!(
        controller.completion_strategy(),
        Ok(CompletionStrategy::Hook)
    );
    assert_eq!(
        controller.transcript(&session),
        Ok(Some(PathBuf::from("/transcripts/session-1")))
    );
    assert_eq!(recording.events(), vec![Event::Transcript(session)]);
}

#[test]
fn shutdown_delegates_once_to_the_transport() {
    let (mut controller, recording, _, _) = controller();

    controller.shutdown().expect("shutdown");

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
