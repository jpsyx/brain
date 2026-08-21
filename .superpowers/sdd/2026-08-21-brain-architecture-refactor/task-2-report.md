# Task 2 report: owned task options and TUI launch boundary

## Implementation

- Added `TaskViewOptions`, an owned runtime model for exactly the task-view
  filters and display settings. It deliberately omits command-only fields such
  as the task CSV override, query tokens, subcommand, and `--no-tui`.
- Converted `build_view` and task chrome to take `TaskViewOptions` instead of
  the clap `tasks::cli::Cli` type.
- Kept clap at the command boundary. `command::tasks::browse` resolves its
  input, converts `&Cli` once, and moves the resolved data into `TuiLaunch`.
- Added owned `TuiLaunch`; `run_tui` now has the single signature
  `run_tui(launch: TuiLaunch)` and has no receiver launch argument.
- Added crate-private `AppInit`. Startup keeps its existing setup side effects,
  resolves configuration, assignment state, database, search state, services,
  and server identity, then calls `App::new(AppInit)`.
- Removed `App`'s lifetime and its retained clap reference. `App` stores owned
  `TaskViewOptions`; every renderer, handler, test fixture, and implementation
  now uses lifetime-free `App` references.
- Added the architecture guard and updated architecture/testing documentation.
- Bumped the crate from `0.71.4` to `0.71.5` in `Cargo.toml` and `Cargo.lock`.

## RED evidence

1. `cargo test --release tasks::view::tests::runtime_options_own_task_view_values_after_cli_changes`
   failed before the DTO existed with `E0432`, unresolved import
   `options::TaskViewOptions`.
2. `cargo test --release --test tui_construction_boundary` failed before the
   TUI refactor because `src/tui/app_sync.rs` still gave `App` a lifetime
   parameter.

## GREEN evidence

- `cargo test --release tasks::view::` passed: 20 task-view tests.
- `cargo test --release tui::` passed: 294 TUI tests.
- `cargo test --release --test tui_construction_boundary` passed: 1 test.
- `cargo clippy --release --all-targets -- -D warnings` passed cleanly.
- `rustfmt --check --edition 2024` over every touched Rust file passed.
- `git diff --check` passed.

## Full-suite result

`cargo test --release` ran to completion with exit status `101`. The only
failure was unrelated to this task:

`tests/workspace_suggestion_selector.rs::suggested_workspace_scoped_commands_carry_the_selector`

The guard recursively treats pre-existing
`src/sync/**/tests_sections/*.rs` test-section files as production source and
flags their test-only backticked `brain sync` literals. Task 2 does not modify
the sync tree or the selector guard. This failure follows the prior task's
test-section renames and was left out of scope.

`cargo fmt --check` also remains red only for the recorded unrelated formatter
drift in existing agent, server, skills, sync, schema, and workspace test files;
all Task 2 Rust files are individually rustfmt-clean.

## Self-review

- Confirmed `rg "App<'|App<'_>" src` has no matches.
- Confirmed `rg "&Cli|with_receiver" src/tui --glob="*.rs"` has no matches.
- Confirmed the only construction signatures are `run_tui(TuiLaunch)` and
  `App::new(AppInit)`.
- Preserved startup side effects in `run_tui` and `App::new`; this task does
  not alter runtime ownership beyond the command-to-runtime seam.
- Did not begin Overlay, palette, receiver, or TuiRuntime redesign work.

## Concern

The pre-existing workspace-suggestion architecture test must be corrected to
exclude the repository's `tests_sections` directories before the full suite can
be green. It is outside Task 2's requested boundary and remains unchanged.
