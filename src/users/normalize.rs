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

/// Validate one canonical bare lowercase mailbox used for outbound delivery.
///
/// This intentionally accepts the common ASCII `addr-spec` subset supported
/// by Brain's configured provider identities. Display syntax, quoted local
/// parts, comments, domain literals, and values needing normalization are not
/// canonical outbound identities.
pub fn validate_canonical_mailbox(value: &str) -> Result<(), NormalizeError> {
    if value.is_empty() || value.len() > 254 || !value.is_ascii() {
        return Err(NormalizeError);
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(NormalizeError);
    };
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local.bytes().all(is_canonical_local_byte)
        || domain.is_empty()
        || domain.len() > 253
        || domain.bytes().any(|byte| byte.is_ascii_uppercase())
        || !domain.split('.').all(is_canonical_domain_label)
    {
        return Err(NormalizeError);
    }
    Ok(())
}

const fn is_canonical_local_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'/'
                | b'='
                | b'?'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
                | b'~'
        )
}

fn is_canonical_domain_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && label
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

/// Trim and ASCII-lowercase an email address without provider-specific
/// rewriting.
pub fn normalize_email(value: &str) -> Result<String, NormalizeError> {
    let trimmed = value.trim();
    let normalized = trimmed.to_ascii_lowercase();
    validate_canonical_mailbox(&normalized)?;
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{normalize_email, normalize_mailbox, validate_canonical_mailbox};

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

    #[test]
    fn canonical_outbound_mailboxes_require_one_bare_lowercase_addr_spec() {
        for mailbox in [
            "brain@example.test",
            "first.last+tag@example.test",
            "brain@sub.example.test",
        ] {
            assert!(
                validate_canonical_mailbox(mailbox).is_ok(),
                "canonical mailbox was rejected"
            );
        }
        for mailbox in [
            "Brain@example.test",
            " brain@example.test",
            "brain@example.test ",
            "Brain <brain@example.test>",
            "brain@example.test>",
            "<brain@example.test",
            ".brain@example.test",
            "brain.@example.test",
            "brain..reply@example.test",
            "brain@@example.test",
            "brain@.example.test",
            "brain@example..test",
            "brain@-example.test",
            "brain@example-.test",
            "brain@example.test-",
            "brain@example_test",
            "brain@example.test\n",
            "brain\u{7f}@example.test",
        ] {
            assert!(
                validate_canonical_mailbox(mailbox).is_err(),
                "noncanonical mailbox was accepted"
            );
        }
    }
}
