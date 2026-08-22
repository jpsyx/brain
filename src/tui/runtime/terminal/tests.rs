use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use anyhow::{Result, anyhow};

use super::{ManagedTerminal, TerminalOps, restore_after_event_loop};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    EnableRaw,
    OpenTty,
    EnterAlternateScreenAndEnableMouse,
    DisableMouseMotion,
    PushKeyboard,
    BuildTerminal,
    PopKeyboard,
    DisableRaw,
    DisableMouseAndLeaveAlternateScreen,
    ShowCursor,
}

type Operations = Rc<RefCell<Vec<Operation>>>;
type FailureScript = Rc<RefCell<VecDeque<Operation>>>;

struct RecordingOps {
    operations: Operations,
    failures: FailureScript,
    keyboard_enabled: bool,
}

impl RecordingOps {
    fn new(operations: Operations, failures: FailureScript, keyboard_enabled: bool) -> Self {
        Self {
            operations,
            failures,
            keyboard_enabled,
        }
    }

    fn record(&self, operation: Operation) -> Result<()> {
        self.operations.borrow_mut().push(operation);
        if self.failures.borrow().front() == Some(&operation) {
            self.failures.borrow_mut().pop_front();
            Err(anyhow!("injected {operation:?} failure"))
        } else {
            Ok(())
        }
    }
}

impl TerminalOps for RecordingOps {
    fn enable_raw_mode(&mut self) -> Result<()> {
        self.record(Operation::EnableRaw)
    }

    fn open_tty(&mut self) -> Result<()> {
        self.record(Operation::OpenTty)
    }

    fn enter_alternate_screen_and_enable_mouse(&mut self) -> Result<()> {
        self.record(Operation::EnterAlternateScreenAndEnableMouse)
    }

    fn disable_mouse_motion_reporting(&mut self) {
        self.record(Operation::DisableMouseMotion).ok();
    }

    fn push_keyboard_enhancement(&mut self) -> bool {
        self.record(Operation::PushKeyboard).ok();
        self.keyboard_enabled
    }

    fn build_terminal(&mut self) -> Result<()> {
        self.record(Operation::BuildTerminal)
    }

    fn pop_keyboard_enhancement(&mut self) -> Result<()> {
        self.record(Operation::PopKeyboard)
    }

    fn disable_raw_mode(&mut self) -> Result<()> {
        self.record(Operation::DisableRaw)
    }

    fn disable_mouse_and_leave_alternate_screen(&mut self) -> Result<()> {
        self.record(Operation::DisableMouseAndLeaveAlternateScreen)
    }

    fn show_cursor(&mut self) -> Result<()> {
        self.record(Operation::ShowCursor)
    }
}

fn recorder(
    failures: impl IntoIterator<Item = Operation>,
    keyboard_enabled: bool,
) -> (RecordingOps, Operations, FailureScript) {
    let operations = Rc::new(RefCell::new(Vec::new()));
    let failures = Rc::new(RefCell::new(failures.into_iter().collect()));
    (
        RecordingOps::new(
            Rc::clone(&operations),
            Rc::clone(&failures),
            keyboard_enabled,
        ),
        operations,
        failures,
    )
}

#[test]
fn acquisition_failure_rolls_back_possibly_enabled_modes_in_existing_safe_order() {
    let cases = [
        (Operation::EnableRaw, vec![Operation::EnableRaw]),
        (
            Operation::OpenTty,
            vec![
                Operation::EnableRaw,
                Operation::OpenTty,
                Operation::DisableRaw,
            ],
        ),
        (
            Operation::EnterAlternateScreenAndEnableMouse,
            vec![
                Operation::EnableRaw,
                Operation::OpenTty,
                Operation::EnterAlternateScreenAndEnableMouse,
                Operation::DisableRaw,
                Operation::DisableMouseAndLeaveAlternateScreen,
            ],
        ),
        (
            Operation::BuildTerminal,
            vec![
                Operation::EnableRaw,
                Operation::OpenTty,
                Operation::EnterAlternateScreenAndEnableMouse,
                Operation::DisableMouseMotion,
                Operation::PushKeyboard,
                Operation::BuildTerminal,
                Operation::PopKeyboard,
                Operation::DisableRaw,
                Operation::DisableMouseAndLeaveAlternateScreen,
            ],
        ),
    ];

    for (failure, expected) in cases {
        let (ops, operations, _) = recorder([failure], true);

        let result = ManagedTerminal::acquire(ops);

        assert!(result.is_err(), "{failure:?} unexpectedly succeeded");
        assert_eq!(*operations.borrow(), expected, "failed at {failure:?}");
    }
}

#[test]
fn orderly_restore_preserves_the_existing_terminal_teardown_sequence() {
    let (ops, operations, _) = recorder([], true);
    let mut terminal = ManagedTerminal::acquire(ops).unwrap();
    operations.borrow_mut().clear();

    terminal.restore().unwrap();

    assert_eq!(
        *operations.borrow(),
        [
            Operation::PopKeyboard,
            Operation::DisableRaw,
            Operation::DisableMouseAndLeaveAlternateScreen,
            Operation::ShowCursor,
        ]
    );
}

#[test]
fn restore_omits_keyboard_pop_when_keyboard_enhancement_was_not_enabled() {
    let (ops, operations, _) = recorder([], false);
    let mut terminal = ManagedTerminal::acquire(ops).unwrap();
    operations.borrow_mut().clear();

    terminal.restore().unwrap();

    assert_eq!(
        *operations.borrow(),
        [
            Operation::DisableRaw,
            Operation::DisableMouseAndLeaveAlternateScreen,
            Operation::ShowCursor,
        ]
    );
}

#[test]
fn repeated_restore_is_idempotent() {
    let (ops, operations, _) = recorder([], true);
    let mut terminal = ManagedTerminal::acquire(ops).unwrap();
    operations.borrow_mut().clear();

    terminal.restore().unwrap();
    let after_first_restore = operations.borrow().clone();
    terminal.restore().unwrap();

    assert_eq!(*operations.borrow(), after_first_restore);
}

#[test]
fn required_restore_error_supersedes_loop_after_all_steps_then_only_failures_retry() {
    let (ops, operations, failures) = recorder([], true);
    let mut terminal = ManagedTerminal::acquire(ops).unwrap();
    operations.borrow_mut().clear();
    failures.borrow_mut().extend([
        Operation::PopKeyboard,
        Operation::DisableRaw,
        Operation::DisableMouseAndLeaveAlternateScreen,
    ]);

    let first_result =
        restore_after_event_loop(Err(anyhow!("event loop failed")), || terminal.restore());

    assert_eq!(
        first_result.unwrap_err().to_string(),
        "injected DisableRaw failure"
    );
    assert_eq!(
        *operations.borrow(),
        [
            Operation::PopKeyboard,
            Operation::DisableRaw,
            Operation::DisableMouseAndLeaveAlternateScreen,
            Operation::ShowCursor,
        ]
    );

    operations.borrow_mut().clear();
    terminal.restore().unwrap();

    assert_eq!(
        *operations.borrow(),
        [
            Operation::PopKeyboard,
            Operation::DisableRaw,
            Operation::DisableMouseAndLeaveAlternateScreen,
        ]
    );

    operations.borrow_mut().clear();
    terminal.restore().unwrap();

    assert!(operations.borrow().is_empty());
}

#[test]
fn optional_keyboard_pop_failure_does_not_supersede_loop_error_and_only_it_is_retried() {
    let (ops, operations, failures) = recorder([], true);
    let mut terminal = ManagedTerminal::acquire(ops).unwrap();
    operations.borrow_mut().clear();
    failures.borrow_mut().push_back(Operation::PopKeyboard);

    let result = restore_after_event_loop(Err(anyhow!("event loop failed")), || terminal.restore());

    assert_eq!(result.unwrap_err().to_string(), "event loop failed");
    assert_eq!(
        *operations.borrow(),
        [
            Operation::PopKeyboard,
            Operation::DisableRaw,
            Operation::DisableMouseAndLeaveAlternateScreen,
            Operation::ShowCursor,
        ]
    );

    operations.borrow_mut().clear();
    terminal.restore().unwrap();

    assert_eq!(*operations.borrow(), [Operation::PopKeyboard]);
}

#[test]
fn drop_continues_best_effort_restoration_without_panicking() {
    let (ops, operations, failures) = recorder([], true);
    let terminal = ManagedTerminal::acquire(ops).unwrap();
    operations.borrow_mut().clear();
    failures.borrow_mut().extend([
        Operation::PopKeyboard,
        Operation::DisableRaw,
        Operation::DisableMouseAndLeaveAlternateScreen,
        Operation::ShowCursor,
    ]);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(terminal)));

    assert!(result.is_ok());
    assert_eq!(
        *operations.borrow(),
        [
            Operation::PopKeyboard,
            Operation::DisableRaw,
            Operation::DisableMouseAndLeaveAlternateScreen,
            Operation::ShowCursor,
        ]
    );
}
