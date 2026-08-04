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
- **Workspace runtime isolation.** `tests/workspace_runtime_isolation.rs`
  is a small integration entry point with cohesive `portable_stores`,
  `runtime_state`, and `env_identity` modules plus shared fixture support. It
  constructs two records and contexts under one temporary home, then performs
  real selected writes through env/config/personalization, session response,
  state DB, TUI and sync locks, sync Journal, Reporter current state/log, and
  CSV-baseline APIs. It snapshots the peer registry record plus portable and
  runtime file trees byte-for-byte, repeats selected writes after changing the
  default, and tests same-name/different-UUID rejection. Focused tests also pin
  alias-to-canonical detached sync arguments after alias removal/default change,
  the typed actor integration env, selected reindex `BRAIN_ROOT`, and
  request-UUID habits GET/POST isolation.
- **Workspace documentation contract.** `tests/workspace_docs.rs` runs the
  compiled binary's root, workspace, and nested alias help, then checks only
  stable command names and selector spellings against the current README/docs.
  It also pins the registry, portable manifest, and UUID-cache locations;
  rejects command-like instructions that write structural `root` through
  `brain env`; and requires the prompt-based/non-sandbox disclaimer plus the
  invariant that changing the default workspace never changes access mode. It
  deliberately avoids snapshots and punctuation-heavy prose.
- **Hook integration.** `tests/hook_integration.rs` runs the real Python
  SessionStart hook against a temporary SQLite DB and the real shell installer
  against temporary homes/roots. It covers the typed workspace/actor
  identity plus session attribution contract, selected-root argument
  and `BRAIN_ROOT` precedence, project-relative commands, actor-scoped session
  rotation, equal opaque IDs with conflicting immutable attribution, schema-v2
  row preservation, and malformed/ambient no-op behavior. The hook-installer
  unit tests live in `src/command/server/receiver/hooks/tests.rs`; they pin the
  exact installed Codex JSON command schema, execute the
  actual configured start and stop commands as one attributed lifecycle, and
  proves stale deployed scripts are refreshed. Further unit tests prove locked
  concurrent JSON mutations retain both workspace registrations and unrelated
  settings, always leave parseable JSON, and preserve original bytes when an
  atomic replacement fails. TUI setup tests prove a held workspace singleton
  prevents hook refresh.
  `tests/stop_hook_actor.rs` proves the stable response ID and actor/channel
  completion contract for a Codex-style `thread_id` payload.
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
- **The session store** (`state.rs`, in-memory SQLite). Scoped resume
  ordering, exact composite-scope `claim` win/lose, registration + `release`
  round-trip, exact composite-scope `reap_dead_locks` with an injected
  pid-liveness predicate, preservation of equal opaque IDs across scopes, the
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
  divider), and `key_to_bytes` (non-semantic key → terminal byte encoding).
  Recording frontend/transport tests cover `AgentController` and its App
  consumers: Enter calls semantic submit, injected work queues after a
  controller-owned two-tick delay, shutdown fires once, agent exit closes only
  the panel, normal and triage panels use the selected adapter, and fallback
  completion captures the transport snapshot with the controller's initiating
  actor/channel before teardown.
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
  does not write remote-only state locally. UUID collision tests prove stable
  winners, mirror-order convergence, idempotence, composite dependency
  and free-text `see_also` rewrites, URL/substr preservation,
  deleted-reference fallback, project reverse-link
  regeneration, whole-operation schema refusal, retryable metadata
  publication, and task/habit counter floors through the real allocator. The
  CSV integration regression verifies an unchanged second pass performs no remote write, and
  `sync/trigger.rs` verifies completed detached children are reaped.
  `sync/check.rs` separately proves schema-aware read-only identity, hybrid
  legacy compatibility, labeled baseline/local/remote parse refusal, themed
  warning output, and byte-stable refusal across every task-related store.
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
- **Real agent-provider behavior.** The Rust suite executes the exact installed
  SessionStart and Stop commands against temporary roots and SQLite databases,
  but it does not launch a real Claude or Codex provider session. Provider event
  emission remains the frontend's documented contract, not behavior Brain can
  manufacture or verify in isolation.
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
| `tests/workspace_runtime_isolation.rs` + `tests/workspace_runtime_isolation/` | Two-workspace portable-store, env-identity, default-change, state, lock, response, and sync-runtime isolation, split by concern with shared fixture support. |
| `tests/workspace_docs.rs` | Stable clap-to-doc workspace commands, selector spellings, storage locations, obsolete root-write rejection, and honest access-language invariants. |
| `tests/phase2_acceptance.rs` | Hermetic composed acceptance fixtures for one portable person selected from two independent machine registries and authenticated inbound identity flowing through `ActorContext` into a real task-script assignment. |
| `tests/todo_script_mutators.rs` | Brain-owned task scripts, including selected-root `BRAIN_ROOT` propagation and isolated actor/workspace environment for every subprocess. |
| `tests/task_schema_migration.rs` + `tasks::schema::transaction_tests` | Temp-only inactive migration fixtures: workspace/kind-scoped deterministic UUIDv5, explicit last-legacy-sync and pre-existing durable-backup-base preconditions, exact durable portable backups, canonical/lexical backup-path separation, strict current-schema detection, row/display-ID preservation, byte-idempotent reruns, injected deep-directory and backup-file parent open/sync failures, immediate journal-temporary cleanup, and crash/failure recovery before and throughout replacement. |
| `tests/task_id_collision_merge.rs` + `sync::csv_merge`/`csv_sync`/`counters` tests | Temp-only and pure fixtures for UUID merge identity, name-aligned headers, deterministic display-ID collision winners/allocation, mirror-order and repeat convergence, pipe/comma `blocked_by`, production-format free-text `see_also` rewrites with URL and substring preservation, deleted-target fallback without marker leaks, project metadata reverse links, retry and local/remote error classification, strict/forward-compatible whole-operation schema policy, no-write refusal, and task/habit next-counter floors. |
| `tests/triage_habits_config.rs` + `tasks::triage_habits` tests | Temp-only managed-definition reconciliation, rename-stable marker identity, CLI/TUI/web mutation guards, strict malformed-config refusal, managed-only and unmanaged-carrier display-reference purge, authenticated workspace-bound journal recovery, interprocess ownership, transaction module-size guards, live-file continuity, fresh re-enable, startup/reindex/repair restoration, suppressed-alert post-sync refresh, palette-enable/startup-refresh interleaving, and injected crashes at internal publication and cleanup boundaries. |
| `skills/todo/scripts/tests/test_workspace_context.py` | Standalone Python subprocess coverage for selected-root-only writes, effective-actor assignment, explicit portable-membership validation, legacy and absent assignment-header migration, empty-CSV schema initialization, missing-context failure, UUIDv4 creation, UUID-preserving edits, fresh habit-occurrence identity with assignment/system-key retention, feature-gated managed triage completion, stale-snapshot refusal for CSV and project metadata, shared-owner serialization, protected removal, concurrent counter allocation, and managed-history garbage collection. |
| `tests/verbose_cli.rs` | End-to-end `--verbose` contract for the compiled binary: stdout mirroring, `/tmp` log-file creation, command/action breadcrumbs, and task CSV load/write logging. |

### Phase 2 migration and convergence matrix

Every row below uses temporary roots, registries, caches, databases, and CSVs.
No test reads or writes a real user workspace.

| Scenario | Evidence |
| --- | --- |
| The same portable person is selected on two simulated machines | `two_machine_registries_select_the_same_portable_person` attaches the same portable workspace from two isolated machine registries and selects the same portable user ID on both. No device-specific identity is generated. |
| Authenticated inbound identity drives default assignment | `authenticated_inbound_actor_drives_default_task_assignment` maps generic email and phone senders to a second workspace member, proves that identity overrides the local user, exports the immutable actor context, and creates real task rows assigned to the inbound actor. |
| Two machines independently create the same display ID | `tests/task_id_collision_merge.rs` gives distinct UUID rows the same display ID, swaps local/remote order, and repeats the merge to prove deterministic convergence. |
| Relationships survive display-ID reconciliation | `tests/task_id_collision_merge.rs` covers composite `blocked_by`, bounded free-text `see_also`, deleted-target fallback, and project metadata reverse links. |
| Legacy rows receive stable migration identity | `tests/task_schema_migration.rs` derives UUIDv5 from workspace UUID, CSV kind, and legacy display ID, then proves byte-idempotent fixture migration with exact backups. The migration interface remains inactive. |
| Disable purges managed history without false-positive loss | `tests/triage_habits_config.rs` removes managed definitions, open rows, completed history, and derived references while preserving same-named unmarked rows and unrelated transcripts. `tasks::triage_habits::purge` limits JSON edits to top-level `tasks[]`, preserves unrelated JSON/text bytes and ambiguous display references, and aborts on malformed JSON, invalid UTF-8, or traversal failures. |
| Re-enable starts fresh | `disabling_purges_every_managed_row_and_derived_reference_then_reenables_fresh` proves exactly two new open managed rows, new UUIDs, and no restored history. |

Phase 2 does not test or claim coordinated migration activation against a real
workspace, advisory access enforcement, agent-controller/OpenCode behavior, or
the final shared-server lease and receiver-routing lifecycle. Those belong to
later roadmap phases.

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

# workspace documentation contract
cargo test --release --test workspace_docs

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
