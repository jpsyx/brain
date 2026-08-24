use super::*;

use crate::agent::{AgentSession, SessionScope};
use crate::main_view::MainView;
use crate::state::{
    EmailLineage, ReceiverConversationIdentity, ReceiverJobState, ReceiverSessionBinding,
};

use super::receiver_durable_support::publish_valid_completion;

fn accept_thread_job(
    app: &App,
    db: &Db,
    thread: &str,
    prompt: &str,
    received_at_unix_ms: u64,
) -> crate::state::ReceiverAcceptance {
    let mut inbound = receiver_job(app, email_actor(), Channel::Email, prompt);
    inbound.job_id = uuid::Uuid::new_v4();
    inbound.received_at_unix_ms = received_at_unix_ms;
    inbound.provider_id = Some(format!("provider-{}", inbound.job_id));
    inbound.authenticated_sender = "member@example.test".to_owned();
    inbound.thread_participants = vec!["member@example.test".to_owned()];
    let identity = ReceiverConversationIdentity::email(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
        EmailLineage::verified(thread).expect("verified thread"),
    );
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept durable thread job")
}

#[test]
fn durable_new_rolls_only_its_conversation_then_launches_following_content_fresh() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    app.shell.show_main_view(MainView::BrainSearch);
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let command = accept_thread_job(&app, &db, "thread-reset", " /NeW\n", 100);
    let following = accept_thread_job(&app, &db, "thread-reset", "after the boundary", 200);
    let unrelated = accept_thread_job(&app, &db, "thread-unrelated", "leave this alone", 300);
    let old_binding = ReceiverSessionBinding::new(AgentKind::Claude, "old-native-session")
        .expect("old native binding");
    assert!(
        db.update_receiver_conversation(
            command.conversation_id(),
            "# Old transcript\n\nPrivate prior context",
            Some(&old_binding),
            50,
        )
        .expect("seed old conversation")
    );
    let unrelated_binding =
        ReceiverSessionBinding::new(AgentKind::Claude, "unrelated-native-session")
            .expect("unrelated native binding");
    assert!(
        db.update_receiver_conversation(
            unrelated.conversation_id(),
            "# Unrelated transcript",
            Some(&unrelated_binding),
            50,
        )
        .expect("seed unrelated conversation")
    );
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.context.workspace().id(),
        email_actor(),
    );
    db.register_receiver_session(
        command.conversation_id(),
        &AgentSession::new("old-native-session").expect("old session"),
        "old-instance",
        42,
        &scope,
    )
    .expect("register old conversation");
    db.register_receiver_session(
        unrelated.conversation_id(),
        &AgentSession::new("unrelated-native-session").expect("unrelated session"),
        "unrelated-instance",
        43,
        &scope,
    )
    .expect("register unrelated conversation");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(command.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    let following_job = db.receiver_job(following.job_id()).unwrap().unwrap();
    assert_ne!(following_job.conversation_id(), command.conversation_id());
    assert_eq!(following_job.state(), ReceiverJobState::Launching);
    let retired = db
        .receiver_conversation(command.conversation_id())
        .unwrap()
        .expect("retired conversation remains durable");
    assert_eq!(
        retired.transcript_markdown(),
        "# Old transcript\n\nPrivate prior context"
    );
    assert_eq!(retired.binding(), Some(&old_binding));
    let fresh = db
        .receiver_conversation(following_job.conversation_id())
        .unwrap()
        .expect("fresh conversation");
    assert!(fresh.transcript_markdown().is_empty());
    assert!(fresh.binding().is_none());
    assert!(
        app.services
            .locked_session_for_instance("old-instance", &scope)
            .is_none()
    );
    let untouched = db
        .receiver_conversation(unrelated.conversation_id())
        .unwrap()
        .expect("unrelated conversation");
    assert_eq!(untouched.transcript_markdown(), "# Unrelated transcript");
    assert_eq!(untouched.binding(), Some(&unrelated_binding));
    assert_eq!(
        app.services
            .locked_session_for_instance("unrelated-instance", &scope)
            .as_deref(),
        Some("unrelated-native-session")
    );
    let specs = transport.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("after the boundary"));
    assert!(!specs[0].command.contains("/NeW"));
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );
}

#[test]
fn durable_restart_cuts_prior_backlog_during_active_run_and_preserves_later_fresh_work() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    app.shell.show_main_view(MainView::BrainSearch);
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let active_job = accept_thread_job(&app, &db, "thread-restart", "active answer", 50);
    let active_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(active_transport.transport());
    app.tick_receiver();
    let active_attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();

    let dropped_same = accept_thread_job(&app, &db, "thread-restart", "drop same thread", 100);
    let dropped_other = accept_thread_job(&app, &db, "thread-other", "drop other thread", 150);
    let restart = accept_thread_job(&app, &db, "thread-restart", " /RESTART\n", 200);
    let survivor = accept_thread_job(&app, &db, "thread-restart", "survives restart", 300);
    let untouched = accept_thread_job(&app, &db, "thread-untouched", "later other work", 400);
    let untouched_binding =
        ReceiverSessionBinding::new(AgentKind::Claude, "untouched-native-session")
            .expect("untouched binding");
    assert!(
        db.update_receiver_conversation(
            untouched.conversation_id(),
            "# Untouched transcript",
            Some(&untouched_binding),
            40,
        )
        .expect("seed untouched conversation")
    );
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.context.workspace().id(),
        email_actor(),
    );
    db.register_receiver_session(
        untouched.conversation_id(),
        &AgentSession::new("untouched-native-session").expect("untouched session"),
        "untouched-instance",
        44,
        &scope,
    )
    .expect("register untouched conversation");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(active_job.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Launching
    );
    assert_eq!(
        db.receiver_job(dropped_same.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Failed
    );
    assert_eq!(
        db.receiver_job(dropped_other.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Failed
    );
    assert_eq!(
        db.receiver_job(restart.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    let survivor_job = db.receiver_job(survivor.job_id()).unwrap().unwrap();
    assert_eq!(survivor_job.state(), ReceiverJobState::Queued);
    assert_ne!(survivor_job.conversation_id(), restart.conversation_id());
    assert_eq!(
        db.receiver_job(untouched.job_id())
            .unwrap()
            .unwrap()
            .state(),
        ReceiverJobState::Queued
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(active_transport.shutdowns(), 0);
    assert!(
        app.services
            .locked_session_for_instance(active_attribution.instance(), active_attribution.scope(),)
            .is_some(),
        "restart must not release the active run's exact lifecycle registration"
    );
    let untouched_conversation = db
        .receiver_conversation(untouched.conversation_id())
        .unwrap()
        .expect("untouched conversation");
    assert_eq!(
        untouched_conversation.transcript_markdown(),
        "# Untouched transcript"
    );
    assert_eq!(untouched_conversation.binding(), Some(&untouched_binding));
    assert_eq!(
        app.services
            .locked_session_for_instance("untouched-instance", &scope)
            .as_deref(),
        Some("untouched-native-session")
    );
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );

    publish_valid_completion(&app, "active answer finished");
    app.tick_receiver();
    let survivor_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(survivor_transport.transport());
    app.tick_receiver();

    assert_eq!(active_transport.shutdowns(), 1);
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        survivor.job_id()
    );
    let specs = survivor_transport.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("survives restart"));
    assert!(!specs[0].command.contains("/RESTART"));
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );
}
