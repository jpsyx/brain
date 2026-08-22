//! RAII ownership for `/dev/tty`, ratatui, and every terminal mode Brain
//! changes while the persistent shell is running.

use std::fs::{File, OpenOptions};

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

trait TerminalOps {
    fn enable_raw_mode(&mut self) -> Result<()>;
    fn open_tty(&mut self) -> Result<()>;
    fn enter_alternate_screen_and_enable_mouse(&mut self) -> Result<()>;
    fn disable_mouse_motion_reporting(&mut self);
    fn push_keyboard_enhancement(&mut self) -> bool;
    fn build_terminal(&mut self) -> Result<()>;
    fn pop_keyboard_enhancement(&mut self) -> Result<()>;
    fn disable_raw_mode(&mut self) -> Result<()>;
    fn disable_mouse_and_leave_alternate_screen(&mut self) -> Result<()>;
    fn show_cursor(&mut self) -> Result<()>;
}

#[derive(Default)]
struct EnabledModes {
    raw: bool,
    alternate_screen_and_mouse: bool,
    keyboard_enhancement: bool,
    cursor_restore: bool,
}

struct ManagedTerminal<O: TerminalOps> {
    ops: O,
    enabled: EnabledModes,
}

impl<O: TerminalOps> ManagedTerminal<O> {
    fn acquire(ops: O) -> Result<Self> {
        let mut session = Self {
            ops,
            enabled: EnabledModes::default(),
        };
        session.acquire_inner()?;
        Ok(session)
    }

    fn acquire_inner(&mut self) -> Result<()> {
        self.ops.enable_raw_mode()?;
        self.enabled.raw = true;

        self.ops.open_tty()?;
        // `execute!` can write the first command before a later write fails.
        // Arm both harmless inverse commands before attempting the pair.
        self.enabled.alternate_screen_and_mouse = true;
        self.ops.enter_alternate_screen_and_enable_mouse()?;

        self.ops.disable_mouse_motion_reporting();
        self.enabled.keyboard_enhancement = self.ops.push_keyboard_enhancement();

        self.ops.build_terminal()?;
        self.enabled.cursor_restore = true;
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        let mut first_error = None;

        if self.enabled.keyboard_enhancement {
            match self.ops.pop_keyboard_enhancement() {
                Ok(()) => self.enabled.keyboard_enhancement = false,
                Err(error) => crate::logging::log(format!(
                    "best-effort terminal keyboard restoration failed: {error:#}"
                )),
            }
        }
        if self.enabled.raw {
            match self.ops.disable_raw_mode() {
                Ok(()) => self.enabled.raw = false,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if self.enabled.alternate_screen_and_mouse {
            match self.ops.disable_mouse_and_leave_alternate_screen() {
                Ok(()) => self.enabled.alternate_screen_and_mouse = false,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if self.enabled.cursor_restore {
            match self.ops.show_cursor() {
                Ok(()) => self.enabled.cursor_restore = false,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }
}

impl<O: TerminalOps> Drop for ManagedTerminal<O> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            crate::logging::log(format!(
                "best-effort terminal restoration failed: {error:#}"
            ));
        }
    }
}

#[cfg(test)]
pub(crate) fn restore_after_event_loop(
    event_loop_result: Result<()>,
    restore: impl FnOnce() -> Result<()>,
) -> Result<()> {
    restore()?;
    event_loop_result
}

#[derive(Default)]
struct SystemTerminalOps {
    control: Option<File>,
    terminal: Option<Terminal<CrosstermBackend<File>>>,
}

impl SystemTerminalOps {
    fn control_mut(&mut self) -> &mut File {
        self.control
            .as_mut()
            .expect("terminal acquisition opens /dev/tty before changing its modes")
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<File>> {
        self.terminal
            .as_mut()
            .expect("a successfully acquired terminal session owns a terminal")
    }
}

impl TerminalOps for SystemTerminalOps {
    fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode()?;
        Ok(())
    }

    fn open_tty(&mut self) -> Result<()> {
        // Full-screen rendering stays on the controlling terminal so stdout
        // remains reserved for intentional short-lived command output.
        self.control = Some(OpenOptions::new().write(true).open("/dev/tty")?);
        Ok(())
    }

    fn enter_alternate_screen_and_enable_mouse(&mut self) -> Result<()> {
        execute!(self.control_mut(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(())
    }

    fn disable_mouse_motion_reporting(&mut self) {
        use std::io::Write as _;

        // `EnableMouseCapture` also enables motion reporting. Brain keeps only
        // button and wheel reporting so native terminal links can still work.
        // This adjustment has no inverse; unsupported terminals ignore it.
        let control = self.control_mut();
        let _ = control.write_all(b"\x1b[?1002l\x1b[?1003l");
        let _ = control.flush();
    }

    fn push_keyboard_enhancement(&mut self) -> bool {
        // This remains a best-effort push rather than a capability probe. A
        // terminal without kitty keyboard enhancements ignores the command.
        execute!(
            self.control_mut(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
        )
        .is_ok()
    }

    fn build_terminal(&mut self) -> Result<()> {
        let backend = CrosstermBackend::new(self.control_mut().try_clone()?);
        self.terminal = Some(Terminal::new(backend)?);
        Ok(())
    }

    fn pop_keyboard_enhancement(&mut self) -> Result<()> {
        execute!(self.control_mut(), PopKeyboardEnhancementFlags)?;
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> Result<()> {
        disable_raw_mode()?;
        Ok(())
    }

    fn disable_mouse_and_leave_alternate_screen(&mut self) -> Result<()> {
        execute!(
            self.control_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<()> {
        self.terminal_mut().show_cursor()?;
        Ok(())
    }
}

#[must_use = "dropping the terminal session restores acquired terminal modes"]
pub(crate) struct TerminalSession {
    managed: ManagedTerminal<SystemTerminalOps>,
}

impl TerminalSession {
    pub(crate) fn acquire() -> Result<Self> {
        ManagedTerminal::acquire(SystemTerminalOps::default()).map(|managed| Self { managed })
    }

    pub(crate) fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<File>> {
        self.managed.ops.terminal_mut()
    }

    pub(crate) fn restore(&mut self) -> Result<()> {
        self.managed.restore()
    }
}

#[cfg(test)]
mod tests;
