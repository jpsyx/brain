use std::sync::LazyLock;

use regex::Regex;

pub(super) fn source_privacy_violations(source: &str) -> Vec<&'static str> {
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
        Regex::new(r"(?i)\b(?:[a-z0-9](?:[a-z0-9-]{0,62})\.)+[a-z][a-z0-9-]{1,62}\b")
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
        if HOST
            .find_iter(&literal)
            .map(|candidate| candidate.as_str())
            .any(|host| {
                !is_reserved_host(host)
                    && !looks_like_filename(host)
                    && has_host_context(source, host)
            })
        {
            violations.push("non-generic host");
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
        .any(|character| matches!(character, '<' | '>' | '{' | '}' | '$' | '%'))
    {
        return true;
    }
    if normalized == "::1" || normalized == "localhost" || normalized.ends_with(".localhost") {
        return true;
    }
    if let Ok(address) = normalized.parse::<std::net::Ipv4Addr>() {
        return address.is_loopback()
            || address.octets()[..3] == [192, 0, 2]
            || address.octets()[..3] == [198, 51, 100]
            || address.octets()[..3] == [203, 0, 113];
    }
    ["test", "example", "invalid"]
        .into_iter()
        .any(|suffix| normalized == suffix || normalized.ends_with(&format!(".{suffix}")))
        || ["example.com", "example.net", "example.org"]
            .into_iter()
            .any(|domain| normalized == domain || normalized.ends_with(&format!(".{domain}")))
}

fn has_host_context(source: &str, host: &str) -> bool {
    source.match_indices(host).any(|(offset, _)| {
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        let context = source[line_start..offset].to_ascii_lowercase();
        [
            "address", "callback", "connect", "domain", "endpoint", "host", "origin", "server",
            "uri", "url", "webhook",
        ]
        .into_iter()
        .any(|marker| context.contains(marker))
    })
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
