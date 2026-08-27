use std::sync::LazyLock;

use regex::Regex;

pub(super) fn source_privacy_violations(
    source: &str,
    reject_bare_hosts: bool,
) -> Vec<&'static str> {
    static HOME_PATH: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:[a-z]:)?/(?:users|home)/(?P<identity>[a-z0-9._{}$%<>-]+)(?:/|$)")
            .expect("home-path privacy regex")
    });
    static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@(?P<domain>[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?)\b",
        )
        .expect("email privacy regex")
    });
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)\b[a-z][a-z0-9+.-]*://(?P<authority>[^/\s"'`]+)"#)
            .expect("URL privacy regex")
    });
    static HOST: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(?:[a-z0-9](?:[a-z0-9-]{0,62})\.)+[a-z][a-z0-9-]{1,62}$")
            .expect("host privacy regex")
    });
    static IPV4: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b").expect("IPv4 privacy regex")
    });

    let mut violations = Vec::new();
    for literal in string_literals(source) {
        let normalized_path = normalized_home_path(&literal);
        if HOME_PATH.captures_iter(&normalized_path).any(|capture| {
            !is_generic_home_identity(
                capture
                    .name("identity")
                    .expect("home identity capture")
                    .as_str(),
            )
        }) {
            violations.push("non-generic home path");
        }
        if EMAIL.captures_iter(&literal).any(|capture| {
            !is_reserved_host(
                capture
                    .name("domain")
                    .expect("email domain capture")
                    .as_str(),
            )
        }) {
            violations.push("non-generic email domain");
        }
        if URL.captures_iter(&literal).any(|capture| {
            let authority = capture
                .name("authority")
                .expect("URL authority capture")
                .as_str();
            authority_host(authority).is_some_and(|host| !is_reserved_host(host))
        }) {
            violations.push("non-generic URL host");
        }
        if reject_bare_hosts {
            if let Some(host) = bare_host(&literal, &HOST) {
                if !(is_reserved_host(host.value)
                    || host.may_be_filename && looks_like_filename(host.value))
                {
                    violations.push("non-generic host");
                }
            }
        }
        if IPV4
            .find_iter(&literal)
            .map(|candidate| candidate.as_str())
            .any(|host| !is_reserved_host(host))
        {
            violations.push("non-generic IPv4 host");
        }
    }
    violations
}

fn normalized_home_path(literal: &str) -> String {
    let mut normalized = literal.replace('\\', "/");
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized
}

fn is_generic_home_identity(identity: &str) -> bool {
    const GENERIC_IDENTITIES: &[&str] = &[
        "test",
        "tester",
        "testing",
        "example",
        "sample",
        "fixture",
        "placeholder",
        "fake",
        "mock",
        "dummy",
        "me",
        "user",
        "username",
        "alice",
        "bob",
    ];
    if identity
        .chars()
        .any(|character| matches!(character, '<' | '>' | '{' | '}' | '$' | '%'))
    {
        return true;
    }
    identity
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|component| !component.is_empty())
        .all(|component| {
            let without_numeric_suffix =
                component.trim_end_matches(|value: char| value.is_ascii_digit());
            GENERIC_IDENTITIES.contains(&without_numeric_suffix)
        })
}

fn authority_host(authority: &str) -> Option<&str> {
    let without_user = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if let Some(bracketed) = without_user.strip_prefix('[') {
        return bracketed.split_once(']').map(|(host, _)| host);
    }
    Some(
        without_user
            .split_once(':')
            .map_or(without_user, |(host, _)| host),
    )
}

fn is_reserved_host(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized
        .chars()
        .any(|character| matches!(character, '<' | '>' | '{' | '}' | '$'))
    {
        return true;
    }
    if normalized == "localhost" || normalized.ends_with(".localhost") {
        return true;
    }
    if let Some(address) = parse_ip_host(&normalized) {
        return match address {
            std::net::IpAddr::V4(address) => {
                address.is_loopback()
                    || address.octets()[..3] == [192, 0, 2]
                    || address.octets()[..3] == [198, 51, 100]
                    || address.octets()[..3] == [203, 0, 113]
            }
            std::net::IpAddr::V6(address) => {
                address.is_loopback()
                    || address.is_unspecified()
                    || address.segments()[..2] == [0x2001, 0x0db8]
            }
        };
    }
    if normalized.contains('%') {
        return true;
    }
    ["test", "example", "invalid"]
        .into_iter()
        .any(|suffix| normalized == suffix || normalized.ends_with(&format!(".{suffix}")))
        || ["example.com", "example.net", "example.org"]
            .into_iter()
            .any(|domain| normalized == domain || normalized.ends_with(&format!(".{domain}")))
}

fn parse_ip_host(host: &str) -> Option<std::net::IpAddr> {
    if let Ok(address) = host.parse::<std::net::Ipv4Addr>() {
        return Some(std::net::IpAddr::V4(address));
    }
    parse_ipv6_host(host).map(std::net::IpAddr::V6)
}

fn parse_ipv6_host(host: &str) -> Option<std::net::Ipv6Addr> {
    if let Ok(address) = host.parse::<std::net::Ipv6Addr>() {
        return Some(address);
    }
    let (address, zone) = host.split_once("%25")?;
    if !is_valid_zone_identifier(zone) {
        return None;
    }
    address.parse::<std::net::Ipv6Addr>().ok()
}

fn is_valid_zone_identifier(zone: &str) -> bool {
    if zone.is_empty() {
        return false;
    }
    let bytes = zone.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            index += 1;
        } else if byte == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            index += 3;
        } else {
            return false;
        }
    }
    true
}

struct BareHost<'source> {
    value: &'source str,
    may_be_filename: bool,
}

fn bare_host<'source>(literal: &'source str, host: &Regex) -> Option<BareHost<'source>> {
    if let Some(candidate) = dns_host(literal, host) {
        return Some(BareHost {
            value: candidate,
            may_be_filename: !literal.ends_with('.'),
        });
    }
    if parse_ipv6_host(literal).is_some() {
        return Some(BareHost {
            value: literal,
            may_be_filename: false,
        });
    }
    let (candidate, port) = literal.rsplit_once(':')?;
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let candidate = candidate
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(candidate);
    (dns_host(candidate, host).is_some() || parse_ipv6_host(candidate).is_some()).then_some(
        BareHost {
            value: candidate.strip_suffix('.').unwrap_or(candidate),
            may_be_filename: false,
        },
    )
}

fn dns_host<'source>(candidate: &'source str, host: &Regex) -> Option<&'source str> {
    let normalized = candidate.strip_suffix('.').unwrap_or(candidate);
    host.is_match(normalized).then_some(normalized)
}

fn looks_like_filename(host: &str) -> bool {
    let extension = host.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| {
        [
            "csv", "db", "html", "js", "json", "jsonl", "lock", "log", "md", "mjs", "py", "rs",
            "sh", "sock", "sqlite", "toml", "ts", "txt",
        ]
        .contains(&extension.to_ascii_lowercase().as_str())
    })
}

fn delimiter_at(bytes: &[u8], index: usize, width: usize, delimiter: u8) -> bool {
    bytes
        .get(index..index + width)
        .is_some_and(|value| value.iter().all(|byte| *byte == delimiter))
}

fn string_literals(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut literals = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some((literal, next)) = raw_string_literal(bytes, index) {
            literals.push(literal);
            index = next;
            continue;
        }
        let delimiter = bytes[index];
        if !matches!(delimiter, b'\'' | b'"' | b'`') {
            index += 1;
            continue;
        }
        let width = if delimiter_at(bytes, index, 3, delimiter) {
            3
        } else {
            1
        };
        let start = index + width;
        let mut cursor = start;
        let mut closed = None;
        while cursor < bytes.len() {
            if width == 1 && delimiter != b'`' && bytes[cursor] == b'\n' {
                break;
            }
            if bytes[cursor] == b'\\' && width == 1 {
                cursor = (cursor + 2).min(bytes.len());
                continue;
            }
            if delimiter_at(bytes, cursor, width, delimiter) {
                closed = Some(cursor);
                break;
            }
            cursor += 1;
        }
        if let Some(end) = closed {
            literals.push(String::from_utf8_lossy(&bytes[start..end]).into_owned());
            index = end + width;
        } else {
            index += 1;
        }
    }
    literals
}

fn raw_string_literal(bytes: &[u8], index: usize) -> Option<(String, usize)> {
    if index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_') {
        return None;
    }
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hash_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    let hash_count = cursor - hash_start;
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    let start = cursor + 1;
    cursor = start;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes
                .get(cursor + 1..cursor + 1 + hash_count)
                .is_some_and(|hashes| hashes.iter().all(|byte| *byte == b'#'))
        {
            return Some((
                String::from_utf8_lossy(&bytes[start..cursor]).into_owned(),
                cursor + 1 + hash_count,
            ));
        }
        cursor += 1;
    }
    None
}

#[cfg(test)]
#[path = "literals/tests.rs"]
mod tests;
