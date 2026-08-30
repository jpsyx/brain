use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::receiver_recovery_authority::DueRecoveryFixture;
use super::*;

use crate::state::ReceiverJobState;
use crate::tui::receiver::{ReceiverCleanupBoundary, ReceiverLaunchBoundary};
use crate::tui::state::ReceiverRunTabError;

#[derive(Clone, Copy)]
enum AmbiguousSpawnCut {
    OwnerStore,
    CommitBeforeWrite,
    CommitAfterVisibleWrite,
    PostAllocationOwnerStore,
}

#[test]
fn successful_spawn_store_ambiguity_keeps_one_controller_until_activation() {
    for cut in [
        AmbiguousSpawnCut::OwnerStore,
        AmbiguousSpawnCut::CommitBeforeWrite,
        AmbiguousSpawnCut::CommitAfterVisibleWrite,
        AmbiguousSpawnCut::PostAllocationOwnerStore,
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut fixture = DueRecoveryFixture::new(&temporary);
        let transport = TransportRecording::default();
        fixture
            .app
            .brain
            .replace_receiver_transport(transport.transport());
        let state_path = fixture.app.context.state_db_path().to_path_buf();
        match cut {
            AmbiguousSpawnCut::OwnerStore => {
                install_store_failure(&mut fixture.app, ReceiverLaunchBoundary::Spawn, &state_path);
            }
            AmbiguousSpawnCut::CommitBeforeWrite => {
                fixture.app.receiver.install_launch_boundary_hook(
                    ReceiverLaunchBoundary::RecoveryLaunchCommit,
                    Box::new({
                        let state_path = state_path.clone();
                        move || {
                            rusqlite::Connection::open(state_path)
                                .expect("launch-commit failure connection")
                                .execute_batch(
                                    "CREATE TRIGGER fail_recovery_launch_commit
                                     BEFORE UPDATE OF state ON receiver_jobs
                                     WHEN NEW.state = 'launched'
                                     BEGIN
                                       SELECT RAISE(FAIL, 'injected launch-commit failure');
                                     END;",
                                )
                                .expect("inject launch-commit failure");
                        }
                    }),
                );
            }
            AmbiguousSpawnCut::CommitAfterVisibleWrite => fixture
                .app
                .services
                .inject_receiver_recovery_commit_visible_error(),
            AmbiguousSpawnCut::PostAllocationOwnerStore => install_store_failure(
                &mut fixture.app,
                ReceiverLaunchBoundary::Allocation,
                &state_path,
            ),
        }

        fixture.app.tick_receiver();

        assert_eq!(transport.launch_specs().len(), 1);
        assert_eq!(transport.shutdowns(), 0);
        assert_spawned_capability(&fixture);
        match cut {
            AmbiguousSpawnCut::OwnerStore | AmbiguousSpawnCut::PostAllocationOwnerStore => {
                restore_store(&state_path);
            }
            AmbiguousSpawnCut::CommitBeforeWrite => {
                rusqlite::Connection::open(&state_path)
                    .expect("launch-commit recovery connection")
                    .execute_batch("DROP TRIGGER fail_recovery_launch_commit;")
                    .expect("remove launch-commit failure");
            }
            AmbiguousSpawnCut::CommitAfterVisibleWrite => {}
        }

        fixture.app.tick_receiver();

        let active = fixture
            .app
            .receiver
            .active_durable_run()
            .expect("the retained controller must finish exact activation");
        assert_eq!(active.claim.job().id(), fixture.accepted.job_id());
        assert_eq!(transport.launch_specs().len(), 1);
        assert_eq!(transport.shutdowns(), 0);
        assert_eq!(
            fixture
                .db
                .receiver_job(fixture.accepted.job_id())
                .expect("load activated recovery")
                .expect("activated recovery")
                .state(),
            ReceiverJobState::Launched
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum CleanupSpawnCut {
    OwnerLost,
    CommitLost,
    AllocationIdExhausted,
    AllocationAlreadyRunning,
    PostAllocationOwnerLost,
}

#[test]
fn successful_spawn_cleanup_failure_retains_exact_fence_until_retry() {
    for cut in [
        CleanupSpawnCut::OwnerLost,
        CleanupSpawnCut::CommitLost,
        CleanupSpawnCut::AllocationIdExhausted,
        CleanupSpawnCut::AllocationAlreadyRunning,
        CleanupSpawnCut::PostAllocationOwnerLost,
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut fixture = DueRecoveryFixture::new(&temporary);
        let later = accept_email_job(&fixture.app, &fixture.db, "later FIFO work", 200);
        let transport = TransportRecording::default();
        fixture
            .app
            .brain
            .replace_receiver_transport(transport.transport());
        fixture
            .app
            .receiver
            .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);
        match cut {
            CleanupSpawnCut::OwnerLost => advance_clock_after(
                &mut fixture.app,
                ReceiverLaunchBoundary::Spawn,
                fixture.clock.clone(),
            ),
            CleanupSpawnCut::CommitLost => {
                let path = fixture.app.context.state_db_path().to_path_buf();
                let job_id = fixture.accepted.job_id();
                fixture.app.receiver.install_launch_boundary_hook(
                    ReceiverLaunchBoundary::RecoveryLaunchCommit,
                    Box::new(move || {
                        rusqlite::Connection::open(path)
                            .expect("lost launch-commit owner connection")
                            .execute(
                                "UPDATE receiver_jobs SET claim_owner = 'changed-owner'
                                 WHERE job_id = ?1",
                                [job_id.to_string()],
                            )
                            .expect("replace launch-commit owner");
                    }),
                );
            }
            CleanupSpawnCut::AllocationIdExhausted => {
                crate::tui::state::exhaust_session_tab_ids(&mut fixture.app.brain);
            }
            CleanupSpawnCut::AllocationAlreadyRunning => fixture
                .app
                .receiver
                .inject_recovery_tab_error(ReceiverRunTabError::AlreadyRunning),
            CleanupSpawnCut::PostAllocationOwnerLost => advance_clock_after(
                &mut fixture.app,
                ReceiverLaunchBoundary::Allocation,
                fixture.clock.clone(),
            ),
        }

        fixture.app.tick_receiver();

        assert_eq!(transport.launch_specs().len(), 1);
        assert_eq!(transport.shutdowns(), 0);
        assert_spawned_capability(&fixture);
        assert_eq!(
            fixture
                .db
                .receiver_job(later.job_id())
                .expect("load blocked FIFO job")
                .expect("blocked FIFO job")
                .state(),
            ReceiverJobState::Queued
        );
        assert!(!claim_recovery_session(
            &fixture,
            "competing-before-cleanup"
        ));

        fixture
            .clock
            .advance(std::time::Duration::from_secs(3 * 60));
        fixture.app.tick_receiver();
        fixture.app.tick_receiver();

        assert_eq!(transport.launch_specs().len(), 1);
        assert_eq!(transport.shutdowns(), 1);
        if matches!(cut, CleanupSpawnCut::CommitLost) {
            assert!(
                fixture
                    .app
                    .receiver
                    .spawned_recovery_durable_run()
                    .is_some()
            );
            assert!(!claim_recovery_session(
                &fixture,
                "competing-after-newer-owner"
            ));
            assert_eq!(
                fixture
                    .db
                    .receiver_job(later.job_id())
                    .expect("load FIFO follower behind changed owner")
                    .expect("FIFO follower behind changed owner")
                    .state(),
                ReceiverJobState::Queued
            );
            continue;
        }
        assert!(
            claim_recovery_session(&fixture, "competing-after-cleanup"),
            "cleanup cut {cut:?} retained the exact session"
        );
        SessionStore::release(&fixture.db, "competing-after-cleanup")
            .expect("release post-cleanup claim");
        let terminal = fixture
            .db
            .receiver_job(fixture.accepted.job_id())
            .expect("load terminal recovery")
            .expect("terminal recovery");
        assert_eq!(terminal.state(), ReceiverJobState::AnswerReady);
        assert!(
            fixture
                .db
                .receiver_delivery_counts()
                .unwrap()
                .answer_ready()
                == 1,
            "spawn cleanup changed the answer-ready count"
        );
        assert!(matches!(
            terminal.last_error(),
            Some("recovery-launch-shutdown" | "recovery-attempt-exhausted")
        ));
    }
}

fn install_store_failure(app: &mut App, boundary: ReceiverLaunchBoundary, path: &Path) {
    let path = path.to_path_buf();
    app.receiver.install_launch_boundary_hook(
        boundary,
        Box::new(move || {
            rusqlite::Connection::open(path)
                .expect("store failure connection")
                .execute_batch(
                    "ALTER TABLE receiver_jobs
                     RENAME TO receiver_jobs_recovery_store_unavailable;",
                )
                .expect("inject store failure");
        }),
    );
}

fn restore_store(path: &Path) {
    rusqlite::Connection::open(path)
        .expect("store restore connection")
        .execute_batch(
            "ALTER TABLE receiver_jobs_recovery_store_unavailable
             RENAME TO receiver_jobs;",
        )
        .expect("restore receiver store");
}

fn advance_clock_after(app: &mut App, boundary: ReceiverLaunchBoundary, clock: ReceiverClock) {
    app.receiver.install_launch_boundary_hook(
        boundary,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
}

fn claim_recovery_session(fixture: &DueRecoveryFixture, owner: &str) -> bool {
    let scope = SessionScope::new(
        AgentKind::Claude,
        fixture.app.context.workspace().id(),
        email_actor(),
    );
    SessionStore::claim(&fixture.db, &fixture.session, owner, 77, &scope)
        .expect("claim exact recovery session")
}

fn assert_spawned_capability(fixture: &DueRecoveryFixture) {
    let spawned = fixture
        .app
        .receiver
        .spawned_recovery_durable_run()
        .expect("successful spawn must retain exact local authority");
    assert_eq!(spawned.claimed.claim.job().id(), fixture.accepted.job_id());
    assert_eq!(spawned.claimed.claim.job().token(), fixture.token);
    assert_eq!(
        spawned.claimed.claim.claim().owner(),
        spawned.claimed.identity.instance()
    );
    assert_eq!(
        spawned.attribution.instance(),
        spawned.claimed.identity.instance()
    );
    assert_eq!(spawned.attribution.registered_session(), &fixture.session);
    assert_eq!(spawned.attribution.scope().agent_kind(), AgentKind::Claude);
    assert_eq!(spawned.pid, i32::try_from(std::process::id()).unwrap_or(0));
}
