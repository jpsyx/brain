//! Thin facade from the TUI launch request to its process-lifetime runtime.

use anyhow::Result;

use crate::tui::TuiLaunch;

pub(crate) fn run_tui(launch: TuiLaunch) -> Result<()> {
    let mut runtime = crate::tui::runtime::TuiRuntime::start(launch)?;
    let run_result = runtime.run();
    runtime.shutdown()?;
    run_result
}

#[cfg(test)]
mod tests;
