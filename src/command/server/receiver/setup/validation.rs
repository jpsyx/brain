use anyhow::Result;

pub(super) fn validate_public_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let authority = trimmed
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("receiver public URL must use HTTPS"))?;
    anyhow::ensure!(
        !authority.is_empty()
            && !authority.contains(['/', '?', '#', '@'])
            && !authority.chars().any(char::is_whitespace)
            && !authority.chars().any(char::is_control),
        "receiver public URL must be an HTTPS origin without a path, query, or fragment"
    );
    validate_authority(authority)?;
    Ok(trimmed.to_owned())
}

pub(super) fn validate_authority(authority: &str) -> Result<()> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            anyhow::bail!("receiver public URL host is invalid");
        };
        anyhow::ensure!(
            host.parse::<std::net::Ipv6Addr>().is_ok(),
            "receiver public URL host is invalid"
        );
        return validate_port_suffix(suffix);
    }
    let (host, port) = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    anyhow::ensure!(
        !host.is_empty()
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            && !host.starts_with(['.', '-'])
            && !host.ends_with(['.', '-']),
        "receiver public URL host is invalid"
    );
    if let Some(port) = port {
        validate_port(port)?;
    }
    Ok(())
}

pub(super) fn validate_port_suffix(suffix: &str) -> Result<()> {
    if suffix.is_empty() {
        return Ok(());
    }
    let port = suffix
        .strip_prefix(':')
        .ok_or_else(|| anyhow::anyhow!("receiver public URL host is invalid"))?;
    validate_port(port)
}

pub(super) fn validate_port(port: &str) -> Result<()> {
    anyhow::ensure!(
        port.parse::<u16>().is_ok_and(|port| port > 0),
        "receiver public URL port is invalid"
    );
    Ok(())
}

pub(super) fn provider_cli_flag(name: &str) -> String {
    match name {
        "brain_receiver_public_url" => "public-url".to_owned(),
        _ => name.replace('_', "-"),
    }
}
