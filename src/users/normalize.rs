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
