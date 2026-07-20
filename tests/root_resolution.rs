//! Integration coverage for brain-root resolution via the public API.
//!
//! `brain_root()` itself reads `$HOME`, the running exe path, and the real
//! `config.json`, so it isn't deterministic in a test harness. We instead
//! prove the two IO-free building blocks it composes from
//! (`parse_config_root` + `expand_tilde_with_home`) behave together the way
//! `brain_root` relies on: read the `root` field, then expand its tilde.

use std::path::{Path, PathBuf};

use brain::paths::{expand_tilde_with_home, parse_config_root};

#[test]
fn configured_tilde_root_expands_against_home() {
    let home = Path::new("/Users/x");
    let configured = parse_config_root(r#"{"root": "~/brain"}"#)
        .unwrap()
        .expect("root present");
    let resolved = expand_tilde_with_home(&configured, home);
    assert_eq!(resolved, PathBuf::from("/Users/x/brain"));
}

#[test]
fn blank_config_falls_back_to_default_home_brain() {
    let home = Path::new("/Users/x");
    // A blank `root` parses to None; the caller then uses $HOME/brain.
    assert!(parse_config_root(r#"{"root": ""}"#).unwrap().is_none());
    let fallback = home.join("brain");
    assert_eq!(fallback, PathBuf::from("/Users/x/brain"));
}

#[test]
fn absolute_configured_root_is_used_verbatim() {
    let home = Path::new("/Users/x");
    let configured = parse_config_root(r#"{"root": "/srv/brain"}"#)
        .unwrap()
        .unwrap();
    assert_eq!(
        expand_tilde_with_home(&configured, home),
        PathBuf::from("/srv/brain")
    );
}
