pub(super) fn is_environment_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn is_http_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
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
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub(super) fn is_valid_http_url(url: &str) -> bool {
    if !url.is_ascii()
        || url.chars().any(char::is_whitespace)
        || url.chars().any(char::is_control)
        || url.contains(['#', '\\'])
    {
        return false;
    }
    let Some(remainder) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = remainder
        .split_once(['/', '?'])
        .map_or(remainder, |(authority, _)| authority);
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    if let Some(ipv6) = authority.strip_prefix('[') {
        let Some((host, port)) = ipv6.split_once(']') else {
            return false;
        };
        return !host.is_empty()
            && host
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b':')
            && (port.is_empty() || port.strip_prefix(':').is_some_and(valid_port));
    }
    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && port.is_none_or(valid_port)
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn is_protected_frontend_environment(name: &str) -> bool {
    matches!(
        name,
        "HOME"
            | "PATH"
            | "SHELL"
            | "USER"
            | "LOGNAME"
            | "LANG"
            | "LC_ALL"
            | "LC_CTYPE"
            | "TMPDIR"
            | "SSH_AUTH_SOCK"
            | "CODEX_HOME"
            | "OPENAI_API_KEY"
            | "ANTHROPIC_API_KEY"
    ) || name.starts_with("BRAIN_")
        || name.starts_with("CODEX_")
        || name.starts_with("CLAUDE_CODE_")
}
