//! The one default that cannot be allowed to drift.

/// A regression guard, not a behavior test.
///
/// `agenda_markdown_dir` falls back to the machine-shared `/tmp`, which
/// `HOME` and `XDG_CONFIG_HOME` isolation does not redirect. Three times
/// during this feature's development a unit test resolved that fallback and
/// rewrote the developer's real agenda for today from a two-row fixture CSV.
/// The `cfg(test)` fallback is what stops it; if someone "simplifies" the two
/// branches back into one, this fails.
#[test]
fn unit_tests_never_fall_back_to_the_machines_shared_tmp() {
    assert_ne!(
        crate::tasks::agenda::io::default_markdown_dir(),
        std::path::PathBuf::from("/tmp"),
        "a unit test must never resolve the machine's real agenda directory"
    );
}
