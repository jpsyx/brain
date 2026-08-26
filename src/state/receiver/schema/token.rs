//! Semantic receiver job-token reconciliation.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;

use super::ReceiverJobToken;

pub(super) fn populate_job_tokens(
    connection: &Connection,
    next_token: &mut impl FnMut() -> ReceiverJobToken,
) -> Result<()> {
    let mut statement =
        connection.prepare("SELECT job_id, job_token FROM receiver_jobs ORDER BY job_id")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let parsed = rows
        .iter()
        .map(|(_, token)| {
            token
                .as_deref()
                .and_then(|value| ReceiverJobToken::parse(value).ok())
        })
        .collect::<Vec<_>>();
    let mut keepers = HashMap::with_capacity(rows.len());
    for (index, token) in parsed.iter().copied().enumerate() {
        let Some(token) = token else { continue };
        keepers
            .entry(token)
            .and_modify(|keeper: &mut usize| {
                let canonical = token.to_string();
                let candidate_is_canonical = rows[index].1.as_deref() == Some(canonical.as_str());
                let keeper_is_canonical = rows[*keeper].1.as_deref() == Some(canonical.as_str());
                if candidate_is_canonical && !keeper_is_canonical {
                    *keeper = index;
                }
            })
            .or_insert(index);
    }
    let mut unavailable = parsed.iter().flatten().copied().collect::<HashSet<_>>();
    let mut replacements = Vec::new();
    for (index, (job_id, _)) in rows.iter().enumerate() {
        if parsed[index].is_some_and(|token| keepers.get(&token) == Some(&index)) {
            continue;
        }
        // Cover every reserved value once, plus one slot for a fresh candidate.
        let attempt_limit = unavailable.len().saturating_add(1);
        let token = (0..attempt_limit)
            .find_map(|_| {
                let candidate = next_token();
                unavailable
                    .insert(candidate)
                    .then_some(candidate.to_string())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "receiver job token allocation exhausted for job {job_id} after \
                     {attempt_limit} attempts"
                )
            })?;
        replacements.push((job_id, token));
    }
    for (job_id, token) in replacements {
        connection.execute(
            "UPDATE receiver_jobs SET job_token = ?1 WHERE job_id = ?2",
            rusqlite::params![token, job_id],
        )?;
    }
    for (index, (job_id, raw)) in rows.iter().enumerate() {
        let Some(token) = parsed[index] else { continue };
        if keepers.get(&token) != Some(&index) {
            continue;
        }
        let canonical = token.to_string();
        if raw.as_deref() != Some(canonical.as_str()) {
            connection.execute(
                "UPDATE receiver_jobs SET job_token = ?1 WHERE job_id = ?2",
                rusqlite::params![canonical, job_id],
            )?;
        }
    }
    Ok(())
}
