use anyhow::Result;

use crate::state::{ReceiverCompletionRequest, ReceiverObservationSet};

use super::super::to_i64;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct StoredEvidence {
    pub(super) lifetime_accepted: Option<i64>,
    pub(super) lifetime_progressing: Option<i64>,
    pub(super) attempt_accepted: Option<i64>,
    pub(super) attempt_progressing: Option<i64>,
    pub(super) latest_progress: Option<i64>,
    pub(super) completed: Option<i64>,
    pub(super) revision: i64,
    pub(super) session_id: Option<String>,
}

pub(super) struct MergedEvidence {
    pub(super) lifetime_accepted: Option<i64>,
    pub(super) lifetime_progressing: Option<i64>,
    pub(super) attempt_accepted: Option<i64>,
    pub(super) attempt_progressing: Option<i64>,
    pub(super) latest_progress: Option<i64>,
    pub(super) completed: i64,
    pub(super) revision: i64,
    pub(super) session_id: Option<String>,
}

pub(super) fn validate_existing_observation(
    stored: &StoredEvidence,
    observation: Option<&ReceiverObservationSet>,
    request: &ReceiverCompletionRequest<'_>,
) -> Result<()> {
    let Some(observation) = observation else {
        return Ok(());
    };
    let revision = to_i64(observation.revision, "receiver observation revision")?;
    let exact = observation.token == request.token
        && observation.instance == request.registration.instance()
        && observation.session_id == request.completed_session.as_str()
        && revision == stored.revision
        && observation
            .accepted_at_unix_ms
            .map(|value| to_i64(value, "receiver accepted observation time"))
            .transpose()?
            == stored.attempt_accepted
        && observation
            .progressing_at_unix_ms
            .map(|value| to_i64(value, "receiver progressing observation time"))
            .transpose()?
            == stored.attempt_progressing
        && observation
            .latest_progress_at_unix_ms
            .map(|value| to_i64(value, "receiver latest-progress observation time"))
            .transpose()?
            == stored.latest_progress
        && observation
            .completed_at_unix_ms
            .map(|value| to_i64(value, "receiver completed observation time"))
            .transpose()?
            == stored.completed;
    anyhow::ensure!(
        exact,
        "receiver completion observation conflicts with durable answer"
    );
    Ok(())
}

pub(super) fn merge_completion_evidence(
    stored: &StoredEvidence,
    observation: Option<&ReceiverObservationSet>,
    request: &ReceiverCompletionRequest<'_>,
    local_completion: i64,
) -> Result<MergedEvidence> {
    validate_timeline(
        stored.lifetime_accepted,
        stored.lifetime_progressing,
        stored.completed,
    )?;
    validate_timeline(
        stored.attempt_accepted,
        stored.attempt_progressing,
        stored.completed,
    )?;
    anyhow::ensure!(
        stored.completed.is_none(),
        "receiver job is already completed"
    );
    let Some(observation) = observation else {
        return Ok(MergedEvidence {
            lifetime_accepted: stored.lifetime_accepted,
            lifetime_progressing: stored.lifetime_progressing,
            attempt_accepted: stored.attempt_accepted,
            attempt_progressing: stored.attempt_progressing,
            latest_progress: stored.latest_progress,
            completed: latest_boundary(
                local_completion,
                stored.attempt_accepted,
                stored.latest_progress.or(stored.attempt_progressing),
            ),
            revision: stored.revision.max(1),
            session_id: Some(request.completed_session.as_str().to_owned()),
        });
    };
    anyhow::ensure!(
        observation.token == request.token
            && observation.instance == request.registration.instance()
            && observation.session_id == request.completed_session.as_str(),
        "receiver completion observation identity mismatch"
    );
    let revision = to_i64(observation.revision, "receiver observation revision")?;
    anyhow::ensure!(
        revision > stored.revision,
        "receiver completion observation is not newer"
    );
    anyhow::ensure!(
        stored.revision == 0
            || stored.session_id.as_deref() == Some(observation.session_id.as_str()),
        "receiver observation session continuity mismatch"
    );
    let accepted = observation
        .accepted_at_unix_ms
        .map(|value| to_i64(value, "receiver accepted observation time"))
        .transpose()?;
    let progressing = observation
        .progressing_at_unix_ms
        .map(|value| to_i64(value, "receiver progressing observation time"))
        .transpose()?;
    let latest_progress = observation
        .latest_progress_at_unix_ms
        .map(|value| to_i64(value, "receiver latest-progress observation time"))
        .transpose()?;
    let completed = observation
        .completed_at_unix_ms
        .map(|value| to_i64(value, "receiver completed observation time"))
        .transpose()?;
    validate_timeline(accepted, progressing, completed)?;
    anyhow::ensure!(
        progressing.is_some() == latest_progress.is_some()
            && progressing
                .zip(latest_progress)
                .is_none_or(|(first, latest)| first <= latest)
            && latest_progress
                .zip(completed)
                .is_none_or(|(latest, completed)| latest <= completed),
        "receiver progress-pulse observation is inconsistent"
    );
    let attempt_accepted = merge_boundary(stored.attempt_accepted, accepted, "accepted")?;
    let attempt_progressing =
        merge_boundary(stored.attempt_progressing, progressing, "progressing")?;
    anyhow::ensure!(
        stored
            .latest_progress
            .zip(latest_progress)
            .is_none_or(|(stored, incoming)| stored <= incoming),
        "receiver latest-progress observation regressed"
    );
    let latest_progress = latest_progress.or(stored.latest_progress);
    validate_timeline(attempt_accepted, attempt_progressing, completed)?;
    let completed = completed
        .unwrap_or_else(|| latest_boundary(local_completion, attempt_accepted, latest_progress));
    anyhow::ensure!(
        attempt_accepted.is_none_or(|accepted| accepted <= completed)
            && attempt_progressing.is_none_or(|progressing| progressing <= completed)
            && latest_progress.is_none_or(|latest| latest <= completed),
        "receiver completion precedes durable lifecycle evidence"
    );
    Ok(MergedEvidence {
        lifetime_accepted: stored.lifetime_accepted.or(attempt_accepted),
        lifetime_progressing: stored.lifetime_progressing.or(attempt_progressing),
        attempt_accepted,
        attempt_progressing,
        latest_progress,
        completed,
        revision,
        session_id: Some(observation.session_id.clone()),
    })
}

fn merge_boundary(stored: Option<i64>, incoming: Option<i64>, label: &str) -> Result<Option<i64>> {
    anyhow::ensure!(
        stored
            .zip(incoming)
            .is_none_or(|(left, right)| left == right),
        "receiver {label} observation conflicts with durable evidence"
    );
    Ok(stored.or(incoming))
}

fn latest_boundary(local: i64, accepted: Option<i64>, progressing: Option<i64>) -> i64 {
    progressing
        .or(accepted)
        .map_or(local, |prior| local.max(prior))
}

fn validate_timeline(
    accepted: Option<i64>,
    progressing: Option<i64>,
    completed: Option<i64>,
) -> Result<()> {
    anyhow::ensure!(
        accepted
            .zip(progressing)
            .is_none_or(|(first, second)| first <= second)
            && accepted
                .zip(completed)
                .is_none_or(|(first, last)| first <= last)
            && progressing
                .zip(completed)
                .is_none_or(|(middle, last)| middle <= last),
        "receiver observation timestamps are not ordered"
    );
    Ok(())
}
