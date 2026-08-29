use anyhow::Result;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension as _};

use crate::state::{
    ReceiverDeliveryAttemptId, ReceiverDeliveryId, ReceiverJobId, ReceiverJobToken,
    ReceiverResponseKind,
};

const DELIVERY_ID_REPAIR_CANDIDATES: usize = 8;

struct PersistedDeliveryShape {
    row_id: i64,
    delivery_id: Value,
    job_id: Value,
    job_token: Value,
    response_kind: Value,
    state: Value,
    attempt_id: Value,
    attempt_count: Value,
    retry_at_unix_ms: Value,
    claim_owner: Value,
    claim_expires_at_unix_ms: Value,
    provider_io_started: Value,
    first_attempt_at_unix_ms: Value,
    error_category: Value,
    ambiguity_reason: Value,
    fallback_decision: Value,
    created_at_unix_ms: Value,
    updated_at_unix_ms: Value,
}

pub(in crate::state::receiver) fn repair_structurally_malformed_deliveries(
    connection: &Connection,
) -> Result<usize> {
    let malformed = {
        let mut statement = connection.prepare(
            "SELECT rowid, delivery_id, job_id, job_token, response_kind, state,
                    attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
                    claim_expires_at_unix_ms, provider_io_started,
                    first_attempt_at_unix_ms, error_category, ambiguity_reason,
                    fallback_decision, created_at_unix_ms, updated_at_unix_ms
             FROM receiver_deliveries ORDER BY rowid",
        )?;
        statement
            .query_map([], |row| {
                Ok(PersistedDeliveryShape {
                    row_id: row.get(0)?,
                    delivery_id: row.get(1)?,
                    job_id: row.get(2)?,
                    job_token: row.get(3)?,
                    response_kind: row.get(4)?,
                    state: row.get(5)?,
                    attempt_id: row.get(6)?,
                    attempt_count: row.get(7)?,
                    retry_at_unix_ms: row.get(8)?,
                    claim_owner: row.get(9)?,
                    claim_expires_at_unix_ms: row.get(10)?,
                    provider_io_started: row.get(11)?,
                    first_attempt_at_unix_ms: row.get(12)?,
                    error_category: row.get(13)?,
                    ambiguity_reason: row.get(14)?,
                    fallback_decision: row.get(15)?,
                    created_at_unix_ms: row.get(16)?,
                    updated_at_unix_ms: row.get(17)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|shape| !shape.is_structurally_valid())
            .collect::<Vec<_>>()
    };
    for shape in &malformed {
        repair_one(connection, shape)?;
    }
    Ok(malformed.len())
}

impl PersistedDeliveryShape {
    fn is_structurally_valid(&self) -> bool {
        required_text(&self.delivery_id)
            .is_some_and(|value| ReceiverDeliveryId::parse(value).is_ok())
            && required_text(&self.job_id).is_some_and(|value| ReceiverJobId::parse(value).is_ok())
            && required_text(&self.job_token)
                .is_some_and(|value| ReceiverJobToken::parse(value).is_ok())
            && required_text(&self.response_kind)
                .is_some_and(|value| ReceiverResponseKind::parse(value).is_some())
            && required_text(&self.state).is_some_and(valid_state)
            && optional_text(&self.attempt_id, |value| {
                ReceiverDeliveryAttemptId::parse(value).is_ok()
            })
            && required_unsigned(&self.attempt_count, i64::from(u32::MAX))
            && optional_unsigned(&self.retry_at_unix_ms)
            && optional_text(&self.claim_owner, |value| !value.trim().is_empty())
            && optional_unsigned(&self.claim_expires_at_unix_ms)
            && matches!(self.provider_io_started, Value::Integer(0 | 1))
            && optional_unsigned(&self.first_attempt_at_unix_ms)
            && optional_text(&self.error_category, valid_error_category)
            && optional_text(&self.ambiguity_reason, valid_ambiguity_reason)
            && optional_text(&self.fallback_decision, valid_fallback_decision)
            && required_unsigned(&self.created_at_unix_ms, i64::MAX)
            && required_unsigned(&self.updated_at_unix_ms, i64::MAX)
    }
}

fn repair_one(connection: &Connection, shape: &PersistedDeliveryShape) -> Result<()> {
    let Some(job_id) = required_text(&shape.job_id) else {
        delete_unrecoverable(connection, shape.row_id)?;
        return Ok(());
    };
    let Some(job_token) = required_text(&shape.job_token) else {
        delete_unrecoverable(connection, shape.row_id)?;
        return Ok(());
    };
    let Some(response_kind) = required_text(&shape.response_kind) else {
        delete_unrecoverable(connection, shape.row_id)?;
        return Ok(());
    };
    if ReceiverJobId::parse(job_id).is_err()
        || ReceiverJobToken::parse(job_token).is_err()
        || ReceiverResponseKind::parse(response_kind).is_none()
        || !exact_job_exists(connection, job_id, job_token)?
    {
        delete_unrecoverable(connection, shape.row_id)?;
        return Ok(());
    }
    let delivery_id = if let Some(delivery_id) =
        required_text(&shape.delivery_id).filter(|value| ReceiverDeliveryId::parse(value).is_ok())
    {
        delivery_id.to_owned()
    } else if let Some(delivery_id) =
        available_repair_delivery_id(connection, shape.row_id, job_id, job_token, response_kind)?
    {
        delivery_id
    } else {
        delete_unrecoverable(connection, shape.row_id)?;
        return Ok(());
    };
    let created_at = nonnegative_or_zero(&shape.created_at_unix_ms);
    let updated_at = nonnegative_or_zero(&shape.updated_at_unix_ms).max(created_at);
    connection.execute(
        "UPDATE receiver_deliveries
         SET delivery_id = ?2, state = 'failed', attempt_id = NULL,
             attempt_count = CASE
               WHEN typeof(attempt_count) = 'integer'
                AND attempt_count BETWEEN 0 AND ?3 THEN attempt_count ELSE 0 END,
             retry_at_unix_ms = NULL, claim_owner = NULL,
             claim_expires_at_unix_ms = NULL, provider_io_started = 0,
             first_attempt_at_unix_ms = NULL, provider_reference = NULL,
             error_category = 'invalid-request', ambiguity_reason = NULL,
             fallback_decision = 'no-safe-fallback',
             created_at_unix_ms = ?4, updated_at_unix_ms = ?5
         WHERE rowid = ?1",
        rusqlite::params![
            shape.row_id,
            delivery_id,
            i64::from(u32::MAX),
            created_at,
            updated_at
        ],
    )?;
    connection.execute(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = 'delivery-invalid-structure',
             updated_at_unix_ms = MAX(updated_at_unix_ms, ?3)
         WHERE job_id = ?1 AND job_token = ?2",
        rusqlite::params![job_id, job_token, updated_at],
    )?;
    Ok(())
}

fn available_repair_delivery_id(
    connection: &Connection,
    row_id: i64,
    job_id: &str,
    job_token: &str,
    response_kind: &str,
) -> Result<Option<String>> {
    for index in 0..DELIVERY_ID_REPAIR_CANDIDATES {
        let seed = if index == 0 {
            format!("{job_id}:{job_token}:{response_kind}")
        } else {
            format!("{job_id}:{job_token}:{response_kind}:repair:{index}")
        };
        let candidate = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, seed.as_bytes()).to_string();
        let occupied = connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM receiver_deliveries
               WHERE delivery_id = ?1 AND rowid != ?2
             )",
            rusqlite::params![candidate, row_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !occupied {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn exact_job_exists(connection: &Connection, job_id: &str, token: &str) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM receiver_jobs WHERE job_id = ?1 AND job_token = ?2",
            rusqlite::params![job_id, token],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false))
}

fn delete_unrecoverable(connection: &Connection, row_id: i64) -> Result<()> {
    connection.execute("DELETE FROM receiver_deliveries WHERE rowid = ?1", [row_id])?;
    Ok(())
}

fn required_text(value: &Value) -> Option<&str> {
    match value {
        Value::Text(value) => Some(value),
        Value::Null | Value::Integer(_) | Value::Real(_) | Value::Blob(_) => None,
    }
}

fn optional_text(value: &Value, validate: impl FnOnce(&str) -> bool) -> bool {
    match value {
        Value::Null => true,
        Value::Text(value) => validate(value),
        Value::Integer(_) | Value::Real(_) | Value::Blob(_) => false,
    }
}

fn required_unsigned(value: &Value, maximum: i64) -> bool {
    matches!(value, Value::Integer(value) if (0..=maximum).contains(value))
}

fn optional_unsigned(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Integer(0..=i64::MAX))
}

fn nonnegative_or_zero(value: &Value) -> i64 {
    match value {
        Value::Integer(value) if *value >= 0 => *value,
        Value::Null | Value::Integer(_) | Value::Real(_) | Value::Text(_) | Value::Blob(_) => 0,
    }
}

fn valid_state(value: &str) -> bool {
    matches!(
        value,
        "cleanup-gated"
            | "ready"
            | "delivering"
            | "retrying"
            | "acknowledged"
            | "failed"
            | "ambiguous"
    )
}

fn valid_error_category(value: &str) -> bool {
    matches!(
        value,
        "authorization"
            | "credentials"
            | "invalid-request"
            | "provider-rejected"
            | "transport-unavailable"
            | "retry-exhausted"
            | "idempotency-window-expired"
    )
}

fn valid_ambiguity_reason(value: &str) -> bool {
    matches!(
        value,
        "provider-acceptance-unknown"
            | "provider-acknowledgement-malformed"
            | "result-commit-unknown"
            | "idempotency-window-expired"
    )
}

fn valid_fallback_decision(value: &str) -> bool {
    matches!(value, "fallback-planned" | "no-safe-fallback")
}
