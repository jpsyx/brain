//! Contact normalization shared by parsing, mutation, and sender resolution.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// A phone or email value cannot be normalized without guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizeError;

impl Display for NormalizeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("identity cannot be normalized unambiguously")
    }
}

impl Error for NormalizeError {}

/// Normalize a phone to E.164.
///
/// Explicit international values must already be `+` followed by 8-15
/// digits. Common ten- or eleven-digit North American formatting is accepted
/// only when the area and exchange codes are unambiguous NANP values.
pub fn normalize_phone(value: &str) -> Result<String, NormalizeError> {
    let trimmed = value.trim();
    if let Some(digits) = trimmed.strip_prefix('+') {
        if (8..=15).contains(&digits.len())
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && !digits.starts_with('0')
        {
            return Ok(format!("+{digits}"));
        }
        return Err(NormalizeError);
    }

    if !trimmed
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b' ' | b'-' | b'(' | b')' | b'.'))
    {
        return Err(NormalizeError);
    }
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    let national = match digits.len() {
        10 => digits.as_str(),
        11 if digits.starts_with('1') => &digits[1..],
        _ => return Err(NormalizeError),
    };
    let bytes = national.as_bytes();
    if !matches!(bytes[0], b'2'..=b'9') || !matches!(bytes[3], b'2'..=b'9') {
        return Err(NormalizeError);
    }
    Ok(format!("+1{national}"))
}

/// Reduce one RFC 5322 mailbox to its bare address, then normalize it.
///
/// Provider values come from real mail headers, which carry
/// `Display Name <someone@example.com>` far more often than a bare address.
/// [`normalize_email`] rejects anything containing whitespace, so inbound
/// values must pass through here before they can be compared with a
/// configured identity. A bare address is returned unchanged.
pub fn normalize_mailbox(value: &str) -> Result<String, NormalizeError> {
    let trimmed = value.trim();
    let address = trimmed
        .rfind('<')
        .and_then(|open| {
            let rest = &trimmed[open + 1..];
            rest.find('>').map(|close| &rest[..close])
        })
        .unwrap_or(trimmed);
    normalize_email(address)
}

/// Trim and ASCII-lowercase an email address without provider-specific
/// rewriting.
pub fn normalize_email(value: &str) -> Result<String, NormalizeError> {
    let trimmed = value.trim();
    let Some((local, domain)) = trimmed.split_once('@') else {
        return Err(NormalizeError);
    };
    if local.is_empty()
        || domain.is_empty()
        || domain.contains('@')
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.chars().any(char::is_control)
    {
        return Err(NormalizeError);
    }
    Ok(trimmed.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::{normalize_email, normalize_mailbox};

    #[test]
    fn a_display_name_mailbox_reduces_to_its_address() {
        assert_eq!(
            normalize_mailbox("Pablo Sarmiento <Pablo@Example.COM>").unwrap(),
            "pablo@example.com"
        );
        assert_eq!(
            normalize_mailbox("\"Sarmiento, Pablo\" <pablo@example.com>").unwrap(),
            "pablo@example.com"
        );
        assert_eq!(
            normalize_mailbox("  pablo@example.com  ").unwrap(),
            "pablo@example.com"
        );
    }

    #[test]
    fn a_non_ascii_display_name_does_not_split_a_character() {
        assert_eq!(
            normalize_mailbox("José Álvarez <jose@example.com>").unwrap(),
            "jose@example.com"
        );
    }

    #[test]
    fn a_mailbox_without_a_usable_address_is_still_rejected() {
        assert!(normalize_mailbox("Pablo Sarmiento").is_err());
        assert!(normalize_mailbox("Pablo <not-an-address>").is_err());
        assert!(normalize_mailbox("").is_err());
    }

    #[test]
    fn plain_normalization_still_refuses_to_guess_at_a_display_name() {
        assert!(normalize_email("Pablo <pablo@example.com>").is_err());
    }
}
