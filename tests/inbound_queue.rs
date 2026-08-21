use brain::server::receiver::{Channel, InboundJob};
use brain::tui::receiver::InboundQueue;

fn job(prompt: &str) -> InboundJob {
    let workspace_id =
        brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let users = brain::users::Users {
        schema_version: brain::users::USERS_SCHEMA_VERSION,
        users: vec![brain::users::User {
            id: brain::users::UserId::parse("member").unwrap(),
            name: "Member".to_owned(),
            phones: vec![brain::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let actor = brain::actor::resolve_actor(
        &brain::users::UserId::parse("member").unwrap(),
        brain::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .unwrap();

    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor,
        channel: Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        prompt: prompt.to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1,
        provider_id: None,
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

fn admit(queue: &mut InboundQueue, prompt: &str) {
    let staged = queue.stage(job(prompt)).expect("queue has room");
    assert!(queue.finalize(staged));
}

fn prompts(queue: &InboundQueue) -> Vec<String> {
    queue.snapshot().into_iter().map(|job| job.prompt).collect()
}

#[test]
fn sixty_four_jobs_are_admitted_and_the_next_is_rejected() {
    let mut queue = InboundQueue::default();

    for index in 0..64 {
        admit(&mut queue, &format!("job {index}"));
    }

    assert!(queue.stage(job("overflow")).is_err());
    assert_eq!(queue.len(), 64);
    assert_eq!(prompts(&queue).first().map(String::as_str), Some("job 0"));
    assert_eq!(prompts(&queue).last().map(String::as_str), Some("job 63"));
}

#[test]
fn a_staged_job_is_hidden_until_its_admission_is_finalized() {
    let mut queue = InboundQueue::default();

    let staged = queue.stage(job("staged")).expect("queue has room");

    assert_eq!(queue.len(), 1, "staged work consumes bounded capacity");
    assert!(queue.head().is_none(), "staged work is not dispatchable");
    assert!(queue.finalize(staged));
    assert_eq!(queue.head().map(|job| job.prompt.as_str()), Some("staged"));
}

#[test]
fn rollback_removes_only_the_exact_staged_tail_job() {
    let mut queue = InboundQueue::default();
    admit(&mut queue, "accepted earlier");
    let staged = queue.stage(job("acknowledgement failed")).unwrap();

    let rolled_back = queue.rollback(staged).expect("staged tail rolls back");

    assert_eq!(rolled_back.prompt, "acknowledgement failed");
    assert_eq!(prompts(&queue), vec!["accepted earlier"]);
}

#[test]
fn a_foreign_token_cannot_finalize_another_queues_staged_tail() {
    let mut first = InboundQueue::default();
    let mut second = InboundQueue::default();
    let first_token = first.stage(job("first staged")).unwrap();
    let second_token = second.stage(job("second staged")).unwrap();

    assert!(!first.finalize(second_token));
    assert!(!second.finalize(first_token));
    assert!(first.head().is_none());
    assert!(second.head().is_none());
}

#[test]
fn a_foreign_token_cannot_roll_back_another_queues_staged_tail() {
    let mut first = InboundQueue::default();
    let mut second = InboundQueue::default();
    let first_token = first.stage(job("first staged")).unwrap();
    let second_token = second.stage(job("second staged")).unwrap();

    assert!(first.rollback(second_token).is_none());
    assert!(second.rollback(first_token).is_none());
    assert_eq!(prompts(&first), vec!["first staged"]);
    assert_eq!(prompts(&second), vec!["second staged"]);
}

#[test]
fn head_commit_is_fifo_and_a_failed_launch_retains_the_head() {
    let mut queue = InboundQueue::default();
    admit(&mut queue, "first");
    admit(&mut queue, "second");
    admit(&mut queue, "third");

    assert_eq!(queue.head().map(|job| job.prompt.as_str()), Some("first"));
    assert!(queue.commit_head(false).is_none());
    assert_eq!(prompts(&queue), vec!["first", "second", "third"]);
    assert_eq!(queue.commit_head(true).unwrap().prompt, "first");
    assert_eq!(prompts(&queue), vec!["second", "third"]);
}

#[test]
fn restart_drops_only_the_backlog_ahead_of_the_command() {
    let mut queue = InboundQueue::default();
    for prompt in ["first", "second", "/restart", "later"] {
        admit(&mut queue, prompt);
    }

    let plan = queue.take_restart().expect("restart command");

    assert_eq!(plan.command.prompt, "/restart");
    assert_eq!(
        plan.dropped
            .into_iter()
            .map(|job| job.prompt)
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
    assert_eq!(prompts(&queue), vec!["later"]);
}

#[test]
fn controls_cannot_consume_the_tail_owned_by_a_staged_token() {
    let mut queue = InboundQueue::default();
    let staged = queue.stage(job("/restart")).expect("queue has room");

    assert!(queue.take_restart().is_none());
    assert_eq!(
        queue
            .rollback(staged)
            .expect("token still owns its tail")
            .prompt,
        "/restart"
    );
    assert!(queue.is_empty());
}

#[test]
fn new_session_is_consumed_only_when_it_reaches_the_head() {
    let mut queue = InboundQueue::default();
    admit(&mut queue, "ordinary");
    admit(&mut queue, "/new");

    assert!(queue.take_new_session().is_none());
    assert_eq!(queue.commit_head(true).unwrap().prompt, "ordinary");
    assert_eq!(queue.take_new_session().unwrap().prompt, "/new");
    assert!(queue.is_empty());
}
