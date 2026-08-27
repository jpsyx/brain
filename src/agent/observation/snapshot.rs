//! One-read schema-v1 snapshot validation and neutral boundary recovery.

use std::fmt::Formatter;

use serde::Deserialize as _;

use super::{
    ACCEPTED_BIT, AgentObservationBoundary, AgentObservationCursor, AgentObservationError,
    AgentObservationPhase, AgentObservationRequest, AgentObservationResult, AgentProgressPulse,
    COMPLETED_BIT, LAUNCHED_BIT, PROGRESSING_BIT, is_canonical_uuid, valid_bounded_identifier,
};

const MAX_SNAPSHOT_BYTES: usize = 4096;

mod file;
use file::read_snapshot_once;
#[cfg(test)]
use file::read_snapshot_once_with_open_hook;

#[cfg(all(test, unix))]
pub(super) fn read_opened_snapshot_for_test(
    body: &[u8],
    declared_length: usize,
    bytes_read: usize,
) -> Result<Vec<u8>, AgentObservationError> {
    file::read_opened_snapshot_for_test(body, declared_length, bytes_read)
}

pub(crate) fn read_normalized_snapshot(
    request: &AgentObservationRequest,
) -> Result<AgentObservationResult, AgentObservationError> {
    normalize_snapshot(request, read_snapshot_once(&request.snapshot_path)?)
}

#[cfg(test)]
pub(super) fn read_normalized_snapshot_with_open_hook(
    request: &AgentObservationRequest,
    before_open: impl FnOnce(),
) -> Result<AgentObservationResult, AgentObservationError> {
    normalize_snapshot(
        request,
        read_snapshot_once_with_open_hook(&request.snapshot_path, before_open)?,
    )
}

fn normalize_snapshot(
    request: &AgentObservationRequest,
    bytes: Option<Vec<u8>>,
) -> Result<AgentObservationResult, AgentObservationError> {
    let Some(bytes) = bytes else {
        return Ok(AgentObservationResult {
            session: request.lifecycle_session.clone(),
            boundaries: Vec::new(),
            progress_pulse: None,
            next_cursor: request.cursor,
        });
    };
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let snapshot = RawSnapshot::deserialize(&mut deserializer)
        .map_err(|_| AgentObservationError::MalformedSnapshot)?;
    deserializer
        .end()
        .map_err(|_| AgentObservationError::MalformedSnapshot)?;
    let parsed = ParsedSnapshot::try_from(snapshot)?;
    if parsed.job_token != request.job_token {
        return Err(AgentObservationError::IdentityMismatch);
    }
    if parsed.instance_id != request.remote_instance {
        return Err(AgentObservationError::IdentityMismatch);
    }
    if parsed.session_id != request.lifecycle_session.as_str() {
        return Err(AgentObservationError::SessionMismatch);
    }
    if parsed.revision < request.cursor.revision {
        return Err(AgentObservationError::RevisionRegression);
    }
    if parsed.revision == request.cursor.revision {
        return Ok(AgentObservationResult {
            session: request.lifecycle_session.clone(),
            boundaries: Vec::new(),
            progress_pulse: None,
            next_cursor: request.cursor,
        });
    }
    let represented = parsed.represented();
    if request.cursor.represented & !represented & !LAUNCHED_BIT != 0 {
        return Err(AgentObservationError::AmbiguousLifecycle);
    }
    if !prior_timestamps_are_unchanged(request.cursor, &parsed)
        || !new_phases_follow_prior_phases(request.cursor, &parsed)
    {
        return Err(AgentObservationError::AmbiguousLifecycle);
    }
    let boundaries: Vec<_> = parsed
        .boundaries()
        .into_iter()
        .filter(|(bit, _)| request.cursor.represented & bit == 0)
        .map(|(_, boundary)| boundary)
        .collect();
    let progress_pulse = match (
        request.cursor.latest_progress_at_unix_ms,
        parsed.latest_progress_at_unix_ms,
    ) {
        (None, Some(observed_at_unix_ms)) => Some(AgentProgressPulse {
            observed_at_unix_ms,
        }),
        (Some(prior), Some(current)) if current > prior => Some(AgentProgressPulse {
            observed_at_unix_ms: current,
        }),
        (Some(prior), Some(current)) if current < prior => {
            return Err(AgentObservationError::AmbiguousLifecycle);
        }
        (Some(_), None) => return Err(AgentObservationError::AmbiguousLifecycle),
        (None, None) | (Some(_), Some(_)) => None,
    };
    if boundaries.is_empty() && progress_pulse.is_none() {
        return Err(AgentObservationError::AmbiguousLifecycle);
    }
    Ok(AgentObservationResult {
        session: request.lifecycle_session.clone(),
        boundaries,
        progress_pulse,
        next_cursor: AgentObservationCursor {
            revision: parsed.revision,
            represented: request.cursor.represented | represented,
            accepted_at_unix_ms: parsed.accepted_at_unix_ms,
            progressing_at_unix_ms: parsed.progressing_at_unix_ms,
            latest_progress_at_unix_ms: parsed.latest_progress_at_unix_ms,
            completed_at_unix_ms: parsed.completed_at_unix_ms,
        },
    })
}

fn new_phases_follow_prior_phases(cursor: AgentObservationCursor, parsed: &ParsedSnapshot) -> bool {
    let prior = [
        cursor.accepted_at_unix_ms.is_some(),
        cursor.progressing_at_unix_ms.is_some(),
        cursor.completed_at_unix_ms.is_some(),
    ];
    let current = [
        parsed.accepted_at_unix_ms.is_some(),
        parsed.progressing_at_unix_ms.is_some(),
        parsed.completed_at_unix_ms.is_some(),
    ];
    current
        .into_iter()
        .enumerate()
        .all(|(index, present)| !present || prior[index] || !prior[index + 1..].contains(&true))
}

fn prior_timestamps_are_unchanged(cursor: AgentObservationCursor, parsed: &ParsedSnapshot) -> bool {
    [
        (cursor.accepted_at_unix_ms, parsed.accepted_at_unix_ms),
        (cursor.progressing_at_unix_ms, parsed.progressing_at_unix_ms),
        (cursor.completed_at_unix_ms, parsed.completed_at_unix_ms),
    ]
    .into_iter()
    .all(|(prior, current)| prior.is_none() || prior == current)
}

struct RawSnapshot {
    version: u64,
    revision: u64,
    phase: String,
    job_token: String,
    instance_id: String,
    session_id: String,
    turn_id: Option<String>,
    accepted_at_unix_ms: Option<u64>,
    progressing_at_unix_ms: Option<u64>,
    latest_progress_at_unix_ms: Option<u64>,
    completed_at_unix_ms: Option<u64>,
}

impl<'de> serde::Deserialize<'de> for RawSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RawSnapshot;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("the exact eleven-field agent observation snapshot")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut version = None;
                let mut revision = None;
                let mut phase = None;
                let mut job_token = None;
                let mut instance_id = None;
                let mut session_id = None;
                let mut turn_id = None;
                let mut accepted_at_unix_ms = None;
                let mut progressing_at_unix_ms = None;
                let mut latest_progress_at_unix_ms = None;
                let mut completed_at_unix_ms = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "version" if version.is_none() => version = Some(map.next_value()?),
                        "revision" if revision.is_none() => revision = Some(map.next_value()?),
                        "phase" if phase.is_none() => phase = Some(map.next_value()?),
                        "job_token" if job_token.is_none() => job_token = Some(map.next_value()?),
                        "instance_id" if instance_id.is_none() => {
                            instance_id = Some(map.next_value()?);
                        }
                        "session_id" if session_id.is_none() => {
                            session_id = Some(map.next_value()?);
                        }
                        "turn_id" if turn_id.is_none() => turn_id = Some(map.next_value()?),
                        "accepted_at_unix_ms" if accepted_at_unix_ms.is_none() => {
                            accepted_at_unix_ms = Some(map.next_value()?);
                        }
                        "progressing_at_unix_ms" if progressing_at_unix_ms.is_none() => {
                            progressing_at_unix_ms = Some(map.next_value()?);
                        }
                        "latest_progress_at_unix_ms" if latest_progress_at_unix_ms.is_none() => {
                            latest_progress_at_unix_ms = Some(map.next_value()?);
                        }
                        "completed_at_unix_ms" if completed_at_unix_ms.is_none() => {
                            completed_at_unix_ms = Some(map.next_value()?);
                        }
                        "version"
                        | "revision"
                        | "phase"
                        | "job_token"
                        | "instance_id"
                        | "session_id"
                        | "turn_id"
                        | "accepted_at_unix_ms"
                        | "progressing_at_unix_ms"
                        | "latest_progress_at_unix_ms"
                        | "completed_at_unix_ms" => {
                            return Err(serde::de::Error::duplicate_field("observation field"));
                        }
                        _ => return Err(serde::de::Error::unknown_field(&field, SNAPSHOT_FIELDS)),
                    }
                }
                Ok(RawSnapshot {
                    version: version.ok_or_else(|| serde::de::Error::missing_field("version"))?,
                    revision: revision
                        .ok_or_else(|| serde::de::Error::missing_field("revision"))?,
                    phase: phase.ok_or_else(|| serde::de::Error::missing_field("phase"))?,
                    job_token: job_token
                        .ok_or_else(|| serde::de::Error::missing_field("job_token"))?,
                    instance_id: instance_id
                        .ok_or_else(|| serde::de::Error::missing_field("instance_id"))?,
                    session_id: session_id
                        .ok_or_else(|| serde::de::Error::missing_field("session_id"))?,
                    turn_id: turn_id.ok_or_else(|| serde::de::Error::missing_field("turn_id"))?,
                    accepted_at_unix_ms: accepted_at_unix_ms
                        .ok_or_else(|| serde::de::Error::missing_field("accepted_at_unix_ms"))?,
                    progressing_at_unix_ms: progressing_at_unix_ms
                        .ok_or_else(|| serde::de::Error::missing_field("progressing_at_unix_ms"))?,
                    latest_progress_at_unix_ms: latest_progress_at_unix_ms.ok_or_else(|| {
                        serde::de::Error::missing_field("latest_progress_at_unix_ms")
                    })?,
                    completed_at_unix_ms: completed_at_unix_ms
                        .ok_or_else(|| serde::de::Error::missing_field("completed_at_unix_ms"))?,
                })
            }
        }

        deserializer.deserialize_map(Visitor)
    }
}

const SNAPSHOT_FIELDS: &[&str] = &[
    "version",
    "revision",
    "phase",
    "job_token",
    "instance_id",
    "session_id",
    "turn_id",
    "accepted_at_unix_ms",
    "progressing_at_unix_ms",
    "latest_progress_at_unix_ms",
    "completed_at_unix_ms",
];

struct ParsedSnapshot {
    revision: i64,
    job_token: String,
    instance_id: String,
    session_id: String,
    accepted_at_unix_ms: Option<u64>,
    progressing_at_unix_ms: Option<u64>,
    latest_progress_at_unix_ms: Option<u64>,
    completed_at_unix_ms: Option<u64>,
}

impl TryFrom<RawSnapshot> for ParsedSnapshot {
    type Error = AgentObservationError;

    fn try_from(raw: RawSnapshot) -> Result<Self, Self::Error> {
        if raw.version != 1
            || raw.revision == 0
            || raw.revision > u64::try_from(i64::MAX).expect("i64 maximum fits u64")
            || !is_canonical_uuid(&raw.job_token)
            || !is_canonical_uuid(&raw.instance_id)
            || !valid_bounded_identifier(&raw.session_id)
            || raw
                .turn_id
                .as_deref()
                .is_some_and(|turn| !valid_bounded_identifier(turn))
        {
            return Err(AgentObservationError::MalformedSnapshot);
        }
        let accepted = raw.accepted_at_unix_ms;
        let progressing = raw.progressing_at_unix_ms;
        let latest_progress = raw.latest_progress_at_unix_ms;
        let completed = raw.completed_at_unix_ms;
        let consistent = match raw.phase.as_str() {
            "accepted" => {
                accepted.is_some()
                    && progressing.is_none()
                    && latest_progress.is_none()
                    && completed.is_none()
            }
            "progressing" => {
                accepted.is_some()
                    && progressing.is_some()
                    && latest_progress.is_some()
                    && completed.is_none()
            }
            "completed" => {
                completed.is_some()
                    && !(accepted.is_none() && progressing.is_some())
                    && (progressing.is_none() == latest_progress.is_none())
            }
            _ => false,
        };
        if !consistent
            || !timestamps_are_nondecreasing(accepted, progressing, latest_progress, completed)
        {
            return Err(AgentObservationError::AmbiguousLifecycle);
        }
        Ok(Self {
            revision: i64::try_from(raw.revision)
                .map_err(|_| AgentObservationError::MalformedSnapshot)?,
            job_token: raw.job_token,
            instance_id: raw.instance_id,
            session_id: raw.session_id,
            accepted_at_unix_ms: accepted,
            progressing_at_unix_ms: progressing,
            latest_progress_at_unix_ms: latest_progress,
            completed_at_unix_ms: completed,
        })
    }
}

impl ParsedSnapshot {
    fn represented(&self) -> u8 {
        (ACCEPTED_BIT * u8::from(self.accepted_at_unix_ms.is_some()))
            | (PROGRESSING_BIT * u8::from(self.progressing_at_unix_ms.is_some()))
            | (COMPLETED_BIT * u8::from(self.completed_at_unix_ms.is_some()))
    }

    fn boundaries(&self) -> Vec<(u8, AgentObservationBoundary)> {
        [
            (
                ACCEPTED_BIT,
                AgentObservationPhase::Accepted,
                self.accepted_at_unix_ms,
            ),
            (
                PROGRESSING_BIT,
                AgentObservationPhase::Progressing,
                self.progressing_at_unix_ms,
            ),
            (
                COMPLETED_BIT,
                AgentObservationPhase::Completed,
                self.completed_at_unix_ms,
            ),
        ]
        .into_iter()
        .filter_map(|(bit, phase, timestamp)| {
            timestamp.map(|observed_at_unix_ms| {
                (
                    bit,
                    AgentObservationBoundary {
                        phase,
                        observed_at_unix_ms,
                    },
                )
            })
        })
        .collect()
    }
}

fn timestamps_are_nondecreasing(
    accepted: Option<u64>,
    progressing: Option<u64>,
    latest_progress: Option<u64>,
    completed: Option<u64>,
) -> bool {
    !(accepted.zip(progressing).is_some_and(|(a, p)| a > p)
        || progressing
            .zip(latest_progress)
            .is_some_and(|(p, latest)| p > latest)
        || latest_progress
            .zip(completed)
            .is_some_and(|(latest, c)| latest > c)
        || accepted.zip(completed).is_some_and(|(a, c)| a > c))
}
