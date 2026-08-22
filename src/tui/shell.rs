//! The `ShellRunner` injection boundary and the URL-open helper.

#[cfg(test)]
use crate::tui::modal_state::FlashKind;

use anyhow::{Context, Result};

/// A side-effecting shell action whose only relevant signal is success
/// vs. failure. Used to inject fakes for the agenda / habits zsh
/// shell-outs in tests.
pub(crate) trait ShellRunner: Send {
    fn run(&self) -> Result<()>;

    /// Open a URL in the user's default handler. Default impl shells out to
    /// macOS `/usr/bin/open <url>` (output discarded; we only care about the
    /// exit status). Lives on `ShellRunner` so the "open link" action
    /// uses the same injectable boundary as the agenda / habits shell-outs —
    /// a test fake records the URL it was asked to open.
    fn open(&self, url: &str) -> Result<()> {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("running `/usr/bin/open {url}`"))?;
        if !status.success() {
            anyhow::bail!("/usr/bin/open {url} exited {status}");
        }
        Ok(())
    }
}

/// Production `ShellRunner`: runs `zsh -ic '<command>'` with output
/// discarded. We need an interactive shell so user-defined functions in
/// `~/.zshrc` (like `agenda`, `habits`) resolve.
pub(crate) struct ZshFunctionRunner {
    command: &'static str,
}

impl ZshFunctionRunner {
    pub const fn new(command: &'static str) -> Self {
        Self { command }
    }
}

impl ShellRunner for ZshFunctionRunner {
    fn run(&self) -> Result<()> {
        let status = std::process::Command::new("zsh")
            .args(["-ic", self.command])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("running `{}` via zsh", self.command))?;
        if !status.success() {
            anyhow::bail!("{} exited {status}", self.command);
        }
        Ok(())
    }
}

/// Open `url` through `runner` and report the `FlashKind` to surface.
/// Factored out so the `ShellRunner::open` hand-off can be unit-tested
/// against a recording fake. Used by both the single-link fast path and
/// the link-picker's selection.
#[cfg(test)]
pub(crate) fn open_url(runner: &dyn ShellRunner, url: &str) -> FlashKind {
    match runner.open(url) {
        Ok(()) => FlashKind::Info(format!("✓ opened {url}")),
        Err(e) => FlashKind::Error(format!("⚠ open failed: {e}")),
    }
}
