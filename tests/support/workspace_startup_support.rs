//! Reproduce the TUI parent's successful startup before server election clocks.
use std::{
    path::Path,
    process::{Command, Stdio},
};

pub fn prepare(home: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "list"])
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("BRAIN_WORKSPACE")
        .env_remove("BRAIN_WORKSPACE_ID")
        .env_remove("BRAIN_REQUIRE_WORKSPACE")
        .env("NO_COLOR", "1")
        .current_dir(home)
        .stdin(Stdio::null())
        .output()
        .expect("prepare server fixture startup");
    assert!(
        output.status.success(),
        "fixture startup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
