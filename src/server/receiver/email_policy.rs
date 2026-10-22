//! Optional recipient domain policy for the machine's shared email endpoint.

use crate::server::workspace_route::WorkspaceRouteError;

pub(crate) const ENV_VAR: &str = "resend_recipient_domain";

/// Normalize an exact domain or a wildcard that requires a subdomain.
pub(crate) fn canonicalize(value: &str) -> anyhow::Result<String> {
    let pattern = value.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return Ok(pattern);
    }
    let domain = pattern.strip_prefix("*.").unwrap_or(&pattern);
    anyhow::ensure!(
        domain.contains('.')
            && crate::users::validate_canonical_mailbox(&format!("policy@{domain}")).is_ok(),
        "resend_recipient_domain must be a domain, *.domain, or empty"
    );
    Ok(pattern)
}

pub(in crate::server) fn filter_destinations(
    configured: Option<&serde_json::Value>,
    mut destinations: Vec<String>,
) -> Result<Vec<String>, WorkspaceRouteError> {
    let Some(configured) = configured else {
        return Ok(destinations);
    };
    let pattern = configured
        .as_str()
        .and_then(|value| canonicalize(value).ok())
        .ok_or_else(|| {
            WorkspaceRouteError::new(500, "invalid resend_recipient_domain configuration")
        })?;
    if pattern.is_empty() {
        return Ok(destinations);
    }
    destinations.retain(|address| allows(&pattern, address));
    if destinations.is_empty() {
        return Err(WorkspaceRouteError::new(
            403,
            "no email recipient satisfies the configured domain policy",
        ));
    }
    Ok(destinations)
}

fn allows(pattern: &str, address: &str) -> bool {
    let Ok(address) = crate::users::normalize_mailbox(address) else {
        return false;
    };
    if crate::users::validate_canonical_mailbox(&address).is_err() {
        return false;
    }
    let Some((_, domain)) = address.rsplit_once('@') else {
        return false;
    };
    pattern.strip_prefix("*.").map_or_else(
        || domain == pattern,
        |suffix| {
            domain
                .strip_suffix(suffix)
                .is_some_and(|prefix| prefix.len() > 1 && prefix.ends_with('.'))
        },
    )
}
