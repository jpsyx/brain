use super::*;

#[test]
fn trailing_dns_root_dot_is_a_host_with_or_without_a_port() {
    for (case_index, literal) in ["receiver.private.lan.", "receiver.private.lan.:8443"]
        .into_iter()
        .enumerate()
    {
        let source = format!(r#"const VALUE: &str = "{literal}";"#);

        assert!(
            source_privacy_violations(&source, true) == vec!["non-generic host"],
            "root-dot host mutation result mismatch at case index {case_index}"
        );
    }
}

#[test]
fn numeric_port_disambiguates_valid_tld_hosts_from_filenames() {
    for (case_index, literal) in [
        "receiver.private.rs:8443",
        "receiver.private.sh:8443",
        "receiver.private.md:8443",
        "receiver.private.py:8443",
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!(r#"const VALUE: &str = "{literal}";"#);

        assert!(
            source_privacy_violations(&source, true) == vec!["non-generic host"],
            "host-port mutation result mismatch at case index {case_index}"
        );
    }
}

#[test]
fn ordinary_repository_paths_and_filenames_are_not_hosts() {
    let source = r#"
        const RUST_MODULE: &str = "receiver.private.rs";
        const PYTHON_SCRIPT: &str = "receiver_observation_bridge.py";
        const NESTED_RUST_MODULE: &str = "src/tui/receiver/private.rs";
        const SHELL_SCRIPT: &str = "hooks.sh";
        const MARKDOWN_FILE: &str = "schema.md";
        const DATABASE_FILE: &str = "state.db";
    "#;

    assert!(
        source_privacy_violations(source, true).is_empty(),
        "ordinary repository path was rejected"
    );
}

#[test]
fn percent_encoded_ipv6_zone_is_not_a_placeholder() {
    for (case_index, literal) in [
        "http://[fe80::1%25en0]:8080/callback",
        "http://[fe80::1%25en%2D0]:8080/callback",
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!(r#"const VALUE: &str = "{literal}";"#);

        assert!(
            source_privacy_violations(&source, true) == vec!["non-generic URL host"],
            "IPv6-zone mutation result mismatch at case index {case_index}"
        );
    }
}

#[test]
fn placeholder_hosts_and_reserved_scoped_ipv6_hosts_remain_allowed() {
    let source = r#"
        const BRACKET_PLACEHOLDER: &str = "http://[fe80::1%ZONE%]:8080/callback";
        const HOST_PLACEHOLDER: &str = "https://%HOST%.private.lan/callback";
        const LOOPBACK_ZONE: &str = "http://[::1%25lo0]:8080/callback";
        const DOCUMENTATION_ZONE: &str = "http://[2001:db8::1%25en0]:8080/callback";
    "#;

    assert!(
        source_privacy_violations(source, true).is_empty(),
        "reserved placeholder host was rejected"
    );
}
