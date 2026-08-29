use std::{os::fd::AsRawFd as _, time::Duration};

use super::{ProbeRunner as _, ShellProbeRunner};

/// A probe runs in its own process group, so an inherited controlling
/// terminal on standard input stops it with `SIGTTIN` and the deadline
/// reports the frontend as unavailable. Probes never read input.
#[test]
fn probe_does_not_inherit_the_parent_standard_input() {
    let (reader, _writer) = nix::unistd::pipe().expect("pipe");
    let saved = nix::unistd::dup(0).expect("save standard input");
    nix::unistd::dup2(reader.as_raw_fd(), 0).expect("redirect standard input");
    let probed = ShellProbeRunner::with_limits(Duration::from_millis(750), 1_024).run("cat", &[]);
    nix::unistd::dup2(saved, 0).expect("restore standard input");
    nix::unistd::close(saved).expect("close saved standard input");

    let output = probed.expect("probe reads no input and exits immediately");
    assert!(output.success);
    assert!(output.stdout.is_empty());
}
