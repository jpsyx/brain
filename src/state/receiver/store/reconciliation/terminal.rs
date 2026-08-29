use anyhow::Result;

use super::recovery_registration::exact_recovery_registration;
use super::support::{EXACT_SNAPSHOT_SQL, ReconciliationCandidate, release_registration};
use crate::state::{
    ReceiverAttemptKind, ReceiverDeliveryState, ReceiverJob, ReceiverJobState,
    ReceiverReconciliationAction, ReceiverReconciliationEffect, ReceiverReconciliationReason,
    ReceiverRecoveryDecision,
};

pub(super) fn terminal_reason(
    job: &ReceiverJob,
    now_unix_ms: u64,
    decision: ReceiverRecoveryDecision,
) -> ReceiverReconciliationReason {
    if decision == ReceiverRecoveryDecision::IncompleteLegacyCompletion {
        return ReceiverReconciliationReason::IncompleteLegacyCompletion;
    }
    if job
        .absolute_work_expires_at_unix_ms()
        .is_some_and(|expiry| now_unix_ms >= expiry)
    {
        return ReceiverReconciliationReason::AbsoluteWorkExpired;
    }
    if job
        .recovery_expires_at_unix_ms()
        .is_some_and(|expiry| now_unix_ms >= expiry)
    {
        return ReceiverReconciliationReason::RecoveryExpired;
    }
    if matches!(
        job.state(),
        ReceiverJobState::Claimed | ReceiverJobState::Launching | ReceiverJobState::Launched
    ) && job.attempt_kind() == ReceiverAttemptKind::Ordinary
    {
        ReceiverReconciliationReason::PreAcceptanceExhausted
    } else {
        ReceiverReconciliationReason::RecoveryExhausted
    }
}

pub(super) fn terminalize(
    transaction: rusqlite::Transaction<'_>,
    workspace_id: &str,
    candidate: &ReconciliationCandidate,
    job: &ReceiverJob,
    reason: ReceiverReconciliationReason,
    now: i64,
    consume_launch_attempt: bool,
) -> Result<Option<ReceiverReconciliationEffect>> {
    terminalize_with_cleanup(
        transaction,
        workspace_id,
        candidate,
        job,
        reason,
        now,
        consume_launch_attempt,
        true,
    )
}

pub(super) fn terminalize_without_observed_cleanup(
    transaction: rusqlite::Transaction<'_>,
    workspace_id: &str,
    candidate: &ReconciliationCandidate,
    job: &ReceiverJob,
    reason: ReceiverReconciliationReason,
    now: i64,
    consume_launch_attempt: bool,
) -> Result<Option<ReceiverReconciliationEffect>> {
    terminalize_with_cleanup(
        transaction,
        workspace_id,
        candidate,
        job,
        reason,
        now,
        consume_launch_attempt,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn terminalize_with_cleanup(
    transaction: rusqlite::Transaction<'_>,
    workspace_id: &str,
    candidate: &ReconciliationCandidate,
    job: &ReceiverJob,
    reason: ReceiverReconciliationReason,
    now: i64,
    consume_launch_attempt: bool,
    include_observed_cleanup: bool,
) -> Result<Option<ReceiverReconciliationEffect>> {
    let pending_cleanup = job
        .recovery_cleanup_instance()
        .zip(job.recovery_cleanup_session_id());
    let observed_cleanup = include_observed_cleanup
        .then(|| job.observation_instance().zip(job.observation_session_id()))
        .flatten();
    let registered_cleanup = if pending_cleanup.is_none()
        && observed_cleanup.is_none()
        && job.attempt_kind() == ReceiverAttemptKind::Recovery
    {
        candidate
            .owner
            .as_deref()
            .map(|instance| {
                exact_recovery_registration(&transaction, workspace_id, job, instance).map(
                    |registration| {
                        registration
                            .map(|registration| (instance.to_owned(), registration.session_id))
                    },
                )
            })
            .transpose()?
            .flatten()
    } else {
        None
    };
    let cleanup_instance = pending_cleanup
        .map(|(instance, _)| instance.to_owned())
        .or_else(|| observed_cleanup.map(|(instance, _)| instance.to_owned()))
        .or_else(|| {
            registered_cleanup
                .as_ref()
                .map(|(instance, _)| instance.clone())
        })
        .or_else(|| {
            include_observed_cleanup
                .then(|| candidate.owner.clone())
                .flatten()
        });
    let cleanup_session_id = pending_cleanup
        .map(|(_, session_id)| session_id.to_owned())
        .or_else(|| observed_cleanup.map(|(_, session_id)| session_id.to_owned()))
        .or_else(|| registered_cleanup.map(|(_, session_id)| session_id));
    let cleanup_is_fenced = cleanup_instance.is_some() && cleanup_session_id.is_some();
    let persisted_cleanup_instance = cleanup_is_fenced
        .then_some(cleanup_instance.as_deref())
        .flatten();
    let persisted_cleanup_session_id = cleanup_is_fenced
        .then_some(cleanup_session_id.as_deref())
        .flatten();
    let notice_state = if cleanup_is_fenced {
        ReceiverDeliveryState::CleanupGated
    } else {
        ReceiverDeliveryState::Ready
    };
    let notice_rendered = insert_unavailable_notice(&transaction, job, notice_state, now)?;
    let terminal_state = if notice_rendered && !cleanup_is_fenced {
        "answer-ready"
    } else {
        "failed"
    };
    let last_error = if notice_rendered {
        reason.as_str()
    } else {
        "notice-no-authorized-destination"
    };
    let sql = format!(
        "UPDATE receiver_jobs
         SET state = ?11, claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_count = retry_count + ?5, retry_at_unix_ms = NULL,
             retry_from_state = NULL, last_error = ?6,
             observation_instance = NULL, observation_session_id = NULL,
             observation_revision = 0, attempt_accepted_at_unix_ms = NULL,
             attempt_progressing_at_unix_ms = NULL,
             latest_progress_at_unix_ms = NULL,
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             recovery_cleanup_instance = ?9,
             recovery_cleanup_session_id = ?10,
             updated_at_unix_ms = ?7
         WHERE workspace_id = ?1 AND job_id = ?2 AND state = ?3
           AND claim_owner IS ?4 AND {EXACT_SNAPSHOT_SQL} = ?8"
    );
    let changed = transaction.execute(
        &sql,
        rusqlite::params![
            workspace_id,
            candidate.job_id.to_string(),
            candidate.state.as_str(),
            candidate.owner,
            i64::from(consume_launch_attempt),
            last_error,
            now,
            candidate.exact_snapshot,
            persisted_cleanup_instance,
            persisted_cleanup_session_id,
            terminal_state,
        ],
    )?;
    if changed != 1 {
        return Ok(None);
    }
    if !cleanup_is_fenced && job.attempt_kind() != ReceiverAttemptKind::Recovery {
        release_registration(
            &transaction,
            workspace_id,
            job.conversation_id(),
            cleanup_instance.as_deref(),
            now,
        )?;
    }
    let queue_depth = agent_queue_depth(&transaction, workspace_id)?;
    transaction.commit()?;
    let phase = if terminal_state == "answer-ready" {
        crate::logging::ReceiverLifecyclePhase::AnswerReady
    } else {
        crate::logging::ReceiverLifecyclePhase::Failed
    };
    crate::logging::log_receiver_lifecycle(crate::logging::ReceiverLifecycleEvent::terminal(
        phase,
        Some(queue_depth),
        lifecycle_reason(reason),
    ));
    Ok(Some(ReceiverReconciliationEffect::new(
        ReceiverReconciliationAction::TerminalFailure,
        reason,
        job.id(),
        job.token(),
        cleanup_instance,
        cleanup_session_id,
    )))
}

pub(super) fn terminalize_launched_recovery(
    transaction: rusqlite::Transaction<'_>,
    workspace_id: &str,
    candidate: &ReconciliationCandidate,
    job: &ReceiverJob,
    reason: ReceiverReconciliationReason,
    now: i64,
) -> Result<Option<ReceiverReconciliationEffect>> {
    let (Some(instance), Some(session_id)) =
        (job.observation_instance(), job.observation_session_id())
    else {
        return Ok(None);
    };
    if job.recovery_cleanup_instance().is_some() || job.recovery_cleanup_session_id().is_some() {
        return Ok(None);
    }
    if !insert_unavailable_notice(&transaction, job, ReceiverDeliveryState::CleanupGated, now)? {
        return Ok(None);
    }
    let sql = format!(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, retry_from_state = NULL, last_error = ?5,
             observation_instance = NULL, observation_session_id = NULL,
             observation_revision = 0, attempt_accepted_at_unix_ms = NULL,
             attempt_progressing_at_unix_ms = NULL,
             latest_progress_at_unix_ms = NULL,
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             recovery_cleanup_instance = ?6,
             recovery_cleanup_session_id = ?7,
             updated_at_unix_ms = ?8
         WHERE workspace_id = ?1 AND job_id = ?2 AND state = 'launched'
           AND claim_owner = ?3 AND attempt_kind = 'recovery'
           AND {EXACT_SNAPSHOT_SQL} = ?4"
    );
    if transaction.execute(
        &sql,
        rusqlite::params![
            workspace_id,
            candidate.job_id.to_string(),
            candidate.owner,
            candidate.exact_snapshot,
            reason.as_str(),
            instance,
            session_id,
            now,
        ],
    )? != 1
    {
        return Ok(None);
    }
    let queue_depth = agent_queue_depth(&transaction, workspace_id)?;
    transaction.commit()?;
    crate::logging::log_receiver_lifecycle(crate::logging::ReceiverLifecycleEvent::terminal(
        crate::logging::ReceiverLifecyclePhase::Failed,
        Some(queue_depth),
        lifecycle_reason(reason),
    ));
    Ok(Some(ReceiverReconciliationEffect::new(
        ReceiverReconciliationAction::TerminalFailure,
        reason,
        job.id(),
        job.token(),
        Some(instance.to_owned()),
        Some(session_id.to_owned()),
    )))
}

fn agent_queue_depth(transaction: &rusqlite::Transaction<'_>, workspace_id: &str) -> Result<usize> {
    transaction
        .query_row(
            "SELECT COUNT(*) FROM receiver_jobs
         WHERE workspace_id = ?1 AND (
           state IN ('queued', 'claimed', 'launching', 'launched', 'accepted', 'processing')
           OR (state = 'retrying' AND retry_from_state IN (
             'claimed', 'launching', 'accepted', 'processing'
           ))
         )",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub(super) const fn lifecycle_reason(
    reason: ReceiverReconciliationReason,
) -> crate::logging::ReceiverLifecycleReason {
    use crate::logging::ReceiverLifecycleReason as Lifecycle;
    match reason {
        ReceiverReconciliationReason::PreAcceptanceTimeout => Lifecycle::PreAcceptanceTimeout,
        ReceiverReconciliationReason::PreAcceptanceExhausted => Lifecycle::PreAcceptanceExhausted,
        ReceiverReconciliationReason::AcceptedStall => Lifecycle::AcceptedStall,
        ReceiverReconciliationReason::AbsoluteWorkExpired => Lifecycle::AbsoluteWorkExpired,
        ReceiverReconciliationReason::RecoveryExpired => Lifecycle::RecoveryExpired,
        ReceiverReconciliationReason::RecoveryExhausted => Lifecycle::RecoveryExhausted,
        ReceiverReconciliationReason::RecoveryPlanningFailed => Lifecycle::RecoveryPlanningFailed,
        ReceiverReconciliationReason::RecoveryRegistrationFailed => {
            Lifecycle::RecoveryRegistrationFailed
        }
        ReceiverReconciliationReason::RecoverySpawnFailed => Lifecycle::RecoverySpawnFailed,
        ReceiverReconciliationReason::RecoveryShutdown => Lifecycle::RecoveryShutdown,
        ReceiverReconciliationReason::NativeSessionUnavailable => {
            Lifecycle::NativeSessionUnavailable
        }
        ReceiverReconciliationReason::IncompleteLegacyCompletion => {
            Lifecycle::IncompleteLegacyCompletion
        }
        ReceiverReconciliationReason::NoticeNoAuthorizedDestination => {
            Lifecycle::NoticeNoAuthorizedDestination
        }
    }
}

pub(super) fn insert_unavailable_notice(
    connection: &rusqlite::Connection,
    job: &ReceiverJob,
    state: ReceiverDeliveryState,
    observed_at_unix_ms: i64,
) -> Result<bool> {
    let notice = crate::server::reply::unanswered_notice(
        super::super::response_intent::channel_label(job.inbound().channel),
    );
    match super::super::response_intent::insert_with_state(
        connection,
        job.id(),
        job.token(),
        job.inbound(),
        crate::state::ReceiverResponseKind::UnavailableNotice,
        &notice.text,
        state,
        observed_at_unix_ms,
    ) {
        Ok(_) => Ok(true),
        Err(error)
            if error
                .downcast_ref::<crate::state::ReceiverDeliveryRenderError>()
                .is_some() =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
