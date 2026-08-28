use crate::state::{
    ReceiverDeliveryAttemptId, ReceiverDeliveryEnvelope, ReceiverDeliveryId, ReceiverJobId,
    ReceiverJobToken, ReceiverProviderCapability,
};

pub(super) struct DueDelivery {
    pub(super) delivery_id: ReceiverDeliveryId,
    pub(super) job_id: ReceiverJobId,
    pub(super) token: ReceiverJobToken,
    pub(super) envelope: ReceiverDeliveryEnvelope,
    pub(super) attempt_count: u32,
    pub(super) first_attempt_at_unix_ms: Option<u64>,
    pub(super) source_state: String,
}

pub(super) struct ExpiredDelivery {
    pub(super) delivery_id: ReceiverDeliveryId,
    pub(super) job_id: ReceiverJobId,
    pub(super) token: ReceiverJobToken,
    pub(super) attempt_id: ReceiverDeliveryAttemptId,
    pub(super) owner: String,
    pub(super) envelope: ReceiverDeliveryEnvelope,
    pub(super) attempt_count: u32,
    pub(super) first_attempt_at_unix_ms: Option<u64>,
    pub(super) provider_io_started: bool,
}

pub(super) fn decode_due_delivery(row: &rusqlite::Row<'_>) -> rusqlite::Result<DueDelivery> {
    decode_delivery_parts(row).map(
        |(delivery_id, job_id, token, envelope, attempt_count, first_attempt_at_unix_ms)| {
            DueDelivery {
                delivery_id,
                job_id,
                token,
                envelope,
                attempt_count,
                first_attempt_at_unix_ms,
                source_state: row.get(6).unwrap_or_default(),
            }
        },
    )
}

type DeliveryParts = (
    ReceiverDeliveryId,
    ReceiverJobId,
    ReceiverJobToken,
    ReceiverDeliveryEnvelope,
    u32,
    Option<u64>,
);

fn decode_delivery_parts(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeliveryParts> {
    let delivery_id = parse_sql(0, &row.get::<_, String>(0)?, ReceiverDeliveryId::parse)?;
    let job_id = parse_sql(1, &row.get::<_, String>(1)?, ReceiverJobId::parse)?;
    let token = parse_sql(2, &row.get::<_, String>(2)?, ReceiverJobToken::parse)?;
    let envelope_json = row.get::<_, String>(3)?;
    let envelope =
        serde_json::from_str(&envelope_json).map_err(|error| sql_decode_error(3, error))?;
    let attempt_count =
        u32::try_from(row.get::<_, i64>(4)?).map_err(|error| sql_decode_error(4, error))?;
    let first_attempt_at_unix_ms = row
        .get::<_, Option<i64>>(5)?
        .map(u64::try_from)
        .transpose()
        .map_err(|error| sql_decode_error(5, error))?;
    Ok((
        delivery_id,
        job_id,
        token,
        envelope,
        attempt_count,
        first_attempt_at_unix_ms,
    ))
}

pub(super) fn decode_expired_delivery(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ExpiredDelivery> {
    let delivery_id = parse_sql(0, &row.get::<_, String>(0)?, ReceiverDeliveryId::parse)?;
    let job_id = parse_sql(1, &row.get::<_, String>(1)?, ReceiverJobId::parse)?;
    let token = parse_sql(2, &row.get::<_, String>(2)?, ReceiverJobToken::parse)?;
    let attempt_id = parse_sql(
        3,
        &row.get::<_, String>(3)?,
        ReceiverDeliveryAttemptId::parse,
    )?;
    let envelope_json = row.get::<_, String>(5)?;
    let envelope =
        serde_json::from_str(&envelope_json).map_err(|error| sql_decode_error(5, error))?;
    Ok(ExpiredDelivery {
        delivery_id,
        job_id,
        token,
        attempt_id,
        owner: row.get(4)?,
        envelope,
        attempt_count: u32::try_from(row.get::<_, i64>(6)?)
            .map_err(|error| sql_decode_error(6, error))?,
        first_attempt_at_unix_ms: row
            .get::<_, Option<i64>>(7)?
            .map(u64::try_from)
            .transpose()
            .map_err(|error| sql_decode_error(7, error))?,
        provider_io_started: row.get(8)?,
    })
}

fn parse_sql<T>(
    index: usize,
    value: &str,
    parse: impl FnOnce(&str) -> anyhow::Result<T>,
) -> rusqlite::Result<T> {
    parse(value).map_err(|_| {
        sql_decode_error(
            index,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "receiver delivery identity is invalid",
            ),
        )
    })
}

pub(super) fn sql_decode_error(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(error))
}

pub(super) fn provider_for(envelope: &ReceiverDeliveryEnvelope) -> ReceiverProviderCapability {
    match envelope {
        ReceiverDeliveryEnvelope::Sms { .. } => ReceiverProviderCapability::Twilio,
        ReceiverDeliveryEnvelope::Email { .. } => ReceiverProviderCapability::Resend,
    }
}
