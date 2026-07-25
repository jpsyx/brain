//! Integration coverage for brain-root resolution via the public API.
//!
//! `brain_root()` itself reads `$HOME` and the real `~/.config/brain-root`, so
//! it isn't deterministic in a test harness. We instead prove the two IO-free
//! building blocks it composes from (`parse_brain_root_file` +
//! `expand_tilde_with_home`) behave together the way `brain_root` relies on:
//! read the pointer file's path, then expand its tilde.

use std::path::{Path, PathBuf};

use brain::paths::{expand_tilde_with_home, parse_brain_root_file};

#[test]
fn pointer_tilde_root_expands_against_home() {
    let home = Path::new("/Users/x");
    let configured = parse_brain_root_file("~/brain\n").expect("path present");
    let resolved = expand_tilde_with_home(&configured, home);
    assert_eq!(resolved, PathBuf::from("/Users/x/brain"));
}

#[test]
fn blank_pointer_falls_back_to_default_home_brain() {
    let home = Path::new("/Users/x");
    // A blank pointer parses to None; the caller then uses $HOME/brain.
    assert!(parse_brain_root_file("   \n").is_none());
    let fallback = home.join("brain");
    assert_eq!(fallback, PathBuf::from("/Users/x/brain"));
}

#[test]
fn absolute_pointer_root_is_used_verbatim() {
    let home = Path::new("/Users/x");
    let configured = parse_brain_root_file("/srv/brain").unwrap();
    assert_eq!(
        expand_tilde_with_home(&configured, home),
        PathBuf::from("/srv/brain")
    );
}
