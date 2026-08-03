# Testing

> **Iron law:** No production code lands without a failing test written
> first. See [AGENTS.md](../AGENTS.md) → "Red/Green TDD" for the contract.

## Red/Green/Refactor — the loop we actually follow

1. **RED.** Write the smallest test that captures the next behavior.
   Run it. Watch it fail (compile error or assertion). A test you never
   saw fail proves nothing.
2. **GREEN.** Write the simplest production code that turns *that* test
   green. Don't add behavior the current red test doesn't demand. Don't
   widen the test surface between red and green.
3. **REFACTOR.** With the bar green, clean up names / structure / dupes.
   Re-run to stay green.

Then the next red. The point of red-first isn't ceremony: it's that a test
which has never failed isn't known to test anything. If a bug ships, the
first move is a failing test that reproduces it, *then* the fix.

## What we test

- **Pure decision logic.** `paths::parse_config_root` (every JSON
  branch), `paths::expand_tilde_with_home`, `open_target::is_textlike`
  (text vs blob, case-insensitivity, extensionless), `finder_target`,
  `open_target::is_markdown` (strictly `.md`) and `pdf_output_path`
  (colocated same-stem `.pdf`).
- **Workspace CLI decisions.** Clap and binary tests cover every placement of
  the raw `--brain/-b` selector, including after delegated task positionals,
  the long equals form, the `--` terminator, and duplicate/missing-value errors,
  plus the complete optional-value management grammar.
  `workspace::command::mutate` tests tilde/relative root normalization and each
  registry-only decision, including a removal variant with no filesystem path
  or delete operation. Hermetic CLI tests force first/later create persistence
  failures and assert exact registry preservation plus preservation of every
  invocation-created root-chain path with actionable manual cleanup. A
  filesystem release barrier starts multiple compiled-binary writers together
  and asserts every successful registration survives. Store tests cover typed
  lock timeouts plus abrupt-exit recovery of a zero-length schema-free lock
  artifact. Provision seams inject partial creation and a replacement path,
  then assert no created directory is removed, only invocation-created paths
  are listed, and the original error remains recoverable through its source
  chain.
  Duplicate same-record aliases are checked case-folded with byte preservation.
  `workspace::command::prompt` tests which missing values require `/dev/tty`,
  plus EOF cancellation, required-value retry, optional blank names,
  multi-answer collection, and contextual read/write failures through injected
  `BufRead`/`Write` seams. Registry-only preflight tests snapshot legacy env and
  pointer bytes plus the complete root/config trees across create/attach EOF,
  prove migration is never called after cancellation, and prove complete flag
  forms perform no terminal IO. The real `/dev/tty` opener stays thin.
- **Workspace bootstrap and readiness.** An exhaustive pure invocation table
  proves every route is context-free, internal-no-prompt, registry-only, or
  ready-workspace. Manifest tests reject unknown fields, schema/version
  incompatibility, and UUID disagreement. Injected `BufRead`/`Write` tests
  prove interactive repair persists and continues the original command, while
  compiled headless tests prove the exact repair commands and that no terminal
  prompt is attempted. Create/attach tests cover manifest ordering/adoption,
  invalid and colliding identities, registry-byte preservation, and manifest
  survival when later registry persistence fails. The first-create regression
  explicitly proves that create leaves `local_user_id` empty and the next
  ordinary command is the setup boundary.
- **The config store (`settings/vars.rs`).** Schema resolution against an explicit
  map (defaults vs overrides — never the real store), the `config list` table
  layout and coloring, value coercion (`4`→number), name normalization, the
  `markdown-to-pdf` prerequisite message wording, and mining an executable path
  out of shell output (a function-wrapper body, ignoring terminal noise).
- **The fuzzy picker's brains.** `HaystackBuf` slug normalization and the
  char→byte highlight map, `char_positions_to_byte_positions`, `refilter`
  (empty query keeps all; substring across stripped slugs; no-hit yields
  no matches; highlight bytes recorded), section grouping in
  `build_display_rows`, and navigation clamping (`move_*`, `page_*`,
  `selected_path`).
- **Menu navigation.** `handle_key` as a pure state machine: movement
  saturation, ctrl-jk, digit jump in/out of range, Enter confirm, Esc /
  Ctrl-c cancel. Plus structural guards: the layout toggle is the last row,
  and every `Choice` appears exactly once (including `CreatePdf` when a
  markdown target is present). Also the two contextual rows: "Create PDF"
  appears only with a `.md` target, leads the list, and carries `^G`;
  "Delete" appears with any target, **trails** the list, and carries `^D`.
  Both elide a long filename through the shared `truncate_label_filename` /
  `LABEL_MAX_FILENAME` (`create_pdf_label` and `delete_label` line up).
- **The confirmation modal** (`confirm.rs`). `handle_key` as a pure state
  machine: PDF is Yes-by-default so Enter accepts while Delete is No-by-default
  so a stray Enter cancels, toggling flips it, `y`/`n` answer directly,
  `Esc`/`Ctrl-c` cancel; plus each `ConfirmKind` carries its own accent
  (green/red), title, and question, the selected button carries the accent
  fill, and the modal shows just the file name.
- **The picker's confirm wiring** (`picker/selection.rs`). `open_confirm` /
  `open_delete_confirm` raise the PDF modal on a `.md` selection (no-op
  otherwise) and the Delete modal on any selection; confirming converts a PDF
  in place or trashes and `drop_path`s the entry (the shell stays open), and
  `reload_entries` / `drop_path` keep the query while updating the list.
- **The palette layout label** (`menu/model.rs`). `layout_choice_label` names the
  opposite side; the toggle row is searchable and appears exactly once.
- **Render helpers.** That `entry_line` preserves the full text, coalesces
  a highlighted run into one correctly-colored span, and paints the
  selection background; that headers/empty-states carry the right text.
- **Filesystem collection** (integration). `entry::collect` against real
  temp trees: bucket tagging, `~/brain/...` rewriting, hidden-file
  skipping, root-skipping, tolerance of a missing bucket.
- **The session store** (`state.rs`, in-memory SQLite). `pick_resume`
  ordering, `claim` win/lose, `register_fresh` + `release` round-trip,
  `reap_dead_locks` with an injected pid-liveness predicate, the
  two-shells-take-distinct-sessions invariant, and `panel_side` round-trip
  + flip. `open_in_memory` / `with_pid_alive` are the test seams
  (deterministic clock + injectable pid probe), so no real process or wall
  clock is involved.
- **Launch builders** (`session.rs`). `AgentKind`, `Plan::decide` (resume vs
  fresh), `build_llm_command` (Claude `--resume`/`--session-id`, Codex `resume`
  and no Claude flags for fresh launches, shell-quoting), and `env_for`.
- **The new-tab opener** (`open_target.rs`). `edit_shell_command` (cd +
  editor, quoting) and `iterm_new_tab_applescript` (embeds the command,
  escapes `"`/`\`).
- **The brain shell's pure bits** (`tui.rs`). `startup_focus` (the shell
  lands in the search panel at startup), `focus_left`/`focus_right`
  (focus follows the layout swap), `panel_borders` (the right panel owns the
  divider), `key_to_bytes` (key → PTY byte encoding), `new_session_bytes`
  (`Ctrl-N` types `/new`, no trailing return), and `advance_submit_countdown`
  (the deferred-Return countdown fires exactly once at zero).
- **Receiver dispatch state.** `tui/receiver_state.rs` proves that an idle
  open panel switches to queued receiver work, an active submitted turn waits,
  a same-channel warm panel is reused, a different channel replaces it, and a
  warm receiver lease never hides interactive Stop-hook completion. Failed
  launches retain their message and retry backoff deadlines are honored.
  `sync/freshness.rs` tests the strict two-hour message threshold;
  `sync/journal.rs` proves push-only/aborted rows do not refresh it.
  `server/delivery.rs` verifies that provider delivery is dispatched off the
  TUI thread.
- **Automatic sync safety.** `sync/args.rs` proves watcher pushes use one-way,
  non-deleting copy arguments; CSV/counter tests prove push-only reconciliation
  does not write remote-only state locally. The CSV integration regression
  verifies an unchanged second pass performs no remote write, and
  `sync/trigger.rs` verifies completed detached children are reaped.
  `tests/watch_local.rs` exercises the real watcher callback in the default
  suite: macOS validates the one-second polling fallback, while other platforms
  use notify's recommended native backend.
- **PTY scrollback** (`pty_pane.rs`). `scroll_up`/`scroll_down` enter and
  clamp scrollback. These spawn a tiny real PTY running `seq` — the one
  place we let a child process in — because it's deterministic and
  sub-second.

## What we deliberately don't test

- **The interactive event loop.** `tui::run_tui` opens `/dev/tty`, toggles raw
  mode, pushes kitty flags, spawns the selected agent PTY, and runs the panel loop. We
  test the *pure* logic it calls (`handle_key`, `App::*`, `focus_*`,
  `panel_borders`, `key_to_bytes`, the render helpers); we don't drive a real
  terminal or a real Claude/Codex process.
- **Ratatui frame output.** We assert on the `Line`s we build, not on
  which cell ratatui painted them into.
- **`std::process::Command` / system `open` / `osascript`.** Spawning
  Finder, the editor tab, or the agent CLI is not a unit. We test the pure builders
  (`finder_target`, `edit_shell_command`, `iterm_new_tab_applescript`,
  `build_llm_command`), not the spawn.
- **The SessionStart hook script.** It's a separate Python process; its
  behavior is covered by a manual smoke test against a temp DB, not the Rust
  suite.
- **Tautological defaults / getters.** `Bucket::Projects.label()` returns
  `"Projects"` — we keep one stability check, not a battery of getter
  tests.
- **"Does it compile" smoke tests.** `cargo build` covers that.

## Test layout

| Location | Scope |
| --- | --- |
| `src/<module>.rs` → `#[cfg(test)] mod tests` | Pure-function unit tests for that module's branches (paths, settings, config, open_target, picker, menu, confirm, render, session, entry). |
| `tests/entry_collect.rs` | `entry::collect` against real temp directory trees. |
| `tests/root_resolution.rs` | `parse_config_root` + `expand_tilde_with_home` composed the way `brain_root` relies on. |
| `tests/workspace_cli.rs` | Compiled-binary workspace registry behavior with isolated `HOME`, `XDG_CONFIG_HOME`, current directory, and roots: manifest-aware create/attach, persistence failures, record-preserving mutations, selector/validation errors, deterministic `NO_COLOR` list output, and non-destructive removal. |
| `tests/workspace_readiness.rs` | Exhaustive bootstrap policy, strict manifest validation, interactive/headless readiness, repair, and first-create-to-next-command flow. |
| `tests/workspace_registry_migration.rs` | Legacy flat-env conversion, exact backups, matching first manifest, idempotence, and persistence-failure preservation. |
| `tests/verbose_cli.rs` | End-to-end `--verbose` contract for the compiled binary: stdout mirroring, `/tmp` log-file creation, command/action breadcrumbs, and task CSV load/write logging. |

`tests/*.rs` reach into the crate via `brain::module::Symbol` because
`src/lib.rs` re-exports the modules. A binary-only crate has no library to
link integration tests against, so `lib.rs` exists purely for that.

## Mocking strategy

We introduce a seam **only** when the real implementation crosses a
process / shell / terminal boundary, and even then we prefer pushing the
logic to a pure function over mocking:

- **No mock for the filesystem.** `entry::collect` runs against real temp
  dirs (`tempfile`). Real walkdir behavior (hidden filter, depth) is the
  spec.
- **No mock for the terminal.** Instead of mocking crossterm, the
  navigation/matching logic is pure (`handle_key`, `App`), and only the
  thin `run()` shell touches `/dev/tty`.
- **No mock for the config store.** `settings` schema resolution runs against
  an explicit in-memory map, never the real `<brain-root>/.config/config.json`.
  That's a value seam, not a mock.

**Production modules don't get test-only methods.** Setup helpers live in
the `#[cfg(test)]` block (unit) or inside the integration test file.

## Commands

```sh
# everything (fast — well under a second)
( cd path/to/brain && cargo test --release )

# one module's unit tests
cargo test --release picker::

# one integration file
cargo test --release --test entry_collect

# lint clean (pedantic + nursery are on)
cargo clippy --release --all-targets

# verbose
cargo test --release -- --nocapture
```

## Adding a test the right way

1. RED first. Add the assertion, build, confirm the failure matches what
   you expect.
2. Implement to GREEN with the minimum code.
3. No new mock unless you cross a process / shell / terminal boundary —
   push the logic to a pure function and test that instead.
4. Setup helpers go under `#[cfg(test)]` or in the test file, never as
   `pub fn make_test_*` on a production module.
5. If the behavior is user-visible (a new key, menu item, or config
   variable), update the relevant `docs/` file in the same change.
