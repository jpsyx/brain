//! External log actions always target this TUI run, independent of main view.

use anyhow::{Context, Result};

use crate::tui::{App, modal_state::FlashKind};

impl App {
    pub(super) fn export_brain_logs(&mut self, copy: bool) {
        let result: Result<()> = (|| {
            let path = self
                .context
                .log_path()
                .context("this run has no log file")?;
            if copy {
                crate::logging::log("copy recent brain logs requested");
                let text = crate::logging::export::recent_logs(path)?;
                crate::open_target::copy_text_to_clipboard(&text)
            } else {
                crate::logging::log("open brain log as text file requested");
                crate::open_target::open_text_file(path)
            }
        })();
        match result {
            Ok(()) => self.status.set_flash(FlashKind::Info(if copy {
                "✓ Copied recent brain logs (up to 2,000 lines)".to_owned()
            } else {
                "✓ Opened brain log file in a text editor".to_owned()
            })),
            Err(error) => {
                crate::logging::log(format!("brain log export failed copy={copy}: {error:#}"));
                self.status.set_error(if self.context.log_path().is_none() {
                    "No log file is available for this Brain run.".to_owned()
                } else if copy {
                    "Could not copy logs. Check clipboard access; choose 'Show brain logs' for details.".to_owned()
                } else {
                    "Could not open the log file. Check your text editor; choose 'Show brain logs' for details.".to_owned()
                });
            }
        }
    }
}
