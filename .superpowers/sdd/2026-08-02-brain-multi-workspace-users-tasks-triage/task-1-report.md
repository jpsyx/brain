# Task 1 report: portable workspace users

## Status

Complete on `feat/workspace-users`. The branch is ready for the parent agent to
review and integrate. It has not been pushed or merged.

## Delivered

- Added strict schema-1 `.config/users.json` parsing, canonical byte-stable
  serialization, same-directory atomic storage, and owner-only temporary-file
  creation.
- Added exact lower-case kebab `UserId` values plus unambiguous phone-to-E.164
  and trimmed ASCII-lowercase email normalization. No provider-specific email
  rewriting occurs.
- Enforced unique user IDs, non-empty names, unique contacts within one user,
  globally unique enabled inbound identities, and response-email membership on
  the same user.
- Added enabled phone/email resolution and a pure, inactive legacy conversion
  proposal. The helper maps only an exact matching response email, leaves other
  old allowlist entries unresolved, allows an ID override, and is not called by
  bootstrap.
- Added `brain user list`, `add`, `update`, `remove`, and `local`, with complete
  noninteractive forms and themed prompts for omitted required values.
- Added atomic removal/reassignment across `users.json`, `tasks/tasks.csv`, and
  `tasks/habits.csv`. Existing `assigned_to` is preferred and legacy `assignee`
  remains readable. Removal refuses assigned work without `--reassign-to`.
- Extended ordinary-command readiness so an existing portable registry must be
  non-empty and the machine-local ID must name one portable person. First
  interactive setup asks for a display name, proposes an ID, creates the person,
  selects it locally, and asks for contacts only for configured channels.
- Preserved legacy behavior intentionally: a workspace with no `users.json` and
  a non-empty prior local ID remains ready and is not automatically migrated.
- Kept person identity separate from device, owner, creator, authentication, and
  audit semantics. Current receiver request handling still uses legacy
  allowlists; inbound actor integration belongs to a later task.
- Split CLI handling from the multi-file removal transaction so each production
  module remains below the repository's approximate 400-line smell threshold.
- Updated architecture, features, data model, configuration, integrations,
  decisions, and docs index. Bumped Brain from `0.17.1` to `0.18.0`.

## TDD evidence

- Initial domain tests failed because `brain::users` did not exist, then passed
  after the schema/store implementation.
- Initial selected CLI tests failed because `user` was unknown, then passed after
  CLI and dispatch implementation.
- Initial readiness tests failed for unknown membership and first-person setup,
  then passed after portable membership and onboarding were implemented.
- The inactive migration-helper and configured-channel prompt tests were each
  observed failing before their implementations.
- The full suite exposed two stale Phase 1 message assertions
  (`root_creation` and `workspace_docs`); those expectations were updated to the
  new `brain user` contract and observed green.

## Verification

- `cargo test --release --test users_store`: 10 passed.
- `cargo test --release --test workspace_readiness`: 12 passed.
- `cargo test --release users::`: 1 matched test passed.
- `cargo test --release bundled_skills_carry_no_personal_data`: passed.
- Isolated `cargo test --release` with configured Python 3.14 first in `PATH`:
  1,116 tests passed, including the five bundled Python script integration tests.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- Explicit `rustfmt --check` over every Task 1 Rust file: passed.
- `git diff --check`: passed.

The first isolated full-suite attempt inherited macOS `/usr/bin/python3` 3.9
first in `PATH`; two unchanged bundled-script tests rejected modern `date |
None` syntax. Repeating with the user's configured Python 3.14 shim first in
`PATH` passed all tests.

## Known repository-level verification issue

`cargo fmt --check` under the installed Rust 1.95 formatter reports extensive
pre-existing formatting drift across unrelated source files. Task 1 files pass
an explicit `rustfmt --check`, and no unrelated formatting was retained. The
repository declares Rust 1.85 but that formatter toolchain is not installed in
this environment.

## Fix round 1 of 5

### Findings addressed

- Legacy response-email migration no longer guesses that an unmatched response
  address belongs to the first portable person. Only a normalized match in the
  legacy email receiver allowlist migrates; unmatched addresses remain explicit
  unresolved inputs.
- A response setting by itself no longer enables email identity or causes the
  first-person readiness flow to prompt for email. Email setup is offered only
  when the receiver email allowlist is configured.
- User removal now uses a recoverable multi-file transaction. It stages and
  syncs mode-preserving replacements and backups, publishes a strict portable
  journal, installs assignment CSVs before `users.json`, and treats journal
  removal as the commit point.
- Ordinary replacement errors restore the complete old generation. Rollback
  failures are returned to the caller and leave the journal/backups recoverable
  for the next load instead of being ignored.
- Process interruption after journal publication is recovered before the next
  `UsersStore::load`. A process interruption during pre-journal staging is also
  cleaned on that recovery path, covering the minor staging-artifact finding.
- The SQLite serialization lock is machine-local and UUID-scoped at
  `~/.cache/brain/workspaces/<workspace-uuid>/users.transaction.lock`; it does
  not add machine state to portable workspace config.
- Grouped replacement preserves each live file's permissions, including an
  owner-only `users.json`.
- The transaction implementation is split into a 321-line coordinator and a
  153-line owned filesystem-primitives child module.
- Brain was bumped from `0.18.0` to `0.18.1`.

### RED evidence

- `legacy_response_email_without_an_allowlist_match_stays_unresolved` failed at
  `proposal.user.response_email.is_none()` because the response address was
  assigned to the first person unconditionally.
- `response_email_alone_does_not_enable_or_prompt_for_an_email_identity` failed
  with `workspace setup cancelled before portable user creation` because setup
  consumed an unexpected email prompt.
- `grouped_removal_preserves_owner_only_users_permissions` failed with mode
  `420` (`0644`) instead of `384` (`0600`).
- The first transaction failure-injection tests failed to compile with an
  unresolved `users::transaction` module before the durable coordinator
  existed.
- The rollback-error injection test required a restore failure step that did
  not exist in the old sequential replacer.
- `different_workspace_ids_never_share_runtime_paths` failed to compile because
  `WorkspacePaths::user_transaction_lock` did not exist. This test captured the
  correction after focused verification found the first lock implementation
  had created `.config/.users.transaction.lock` in portable state.
- `recovery_removes_pre_journal_artifacts_left_by_an_interruption` failed because
  `.brain-user-*` staging artifacts remained after recovery when the simulated
  process stopped before journal publication.

### GREEN evidence

- `cargo test --release --test users_store`: 12 passed.
- `cargo test --release --test workspace_readiness`: 13 passed.
- `cargo test --release users::store::transaction_tests`: 5 passed.
- `different_workspace_ids_never_share_runtime_paths`: passed.
- `bundled_skills_carry_no_personal_data`: passed.
- Final isolated `cargo test --release`, with isolated `HOME`, XDG config/cache,
  `TMPDIR`, and Python 3.14 first in `PATH`: 1,124 tests passed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- Explicit `rustfmt --check` over every fix-round Rust file: passed.
- `git diff --check`: passed.

The first isolated full-suite run used macOS `/usr/bin/python3` 3.9 and failed
two unchanged bundled-script tests on the Python 3.10 `date | None` syntax.
Repeating the same isolated run with `/opt/homebrew/bin` first in `PATH` selected
Python 3.14 and passed all 1,124 tests.

Repository-wide `cargo fmt --check` still reports the same extensive unrelated
Rust 1.95 formatter drift documented above. No unrelated formatting was
retained, and every file changed in this fix round passes an explicit formatter
check.

### Files changed in this fix round

- `Cargo.toml`, `Cargo.lock`
- `src/users/{command,mod,store,transaction,validate}.rs`
- `src/users/transaction/files.rs`
- `src/command/users/removal.rs`
- `src/workspace/{bootstrap,mod,paths}.rs`
- `tests/users_store.rs`, `tests/workspace_readiness.rs`
- `docs/{architecture,config,data-model,decisions,features}.md`

Commit: the fix-round branch HEAD created immediately after this report; no push
or merge was performed.

## Fix round 2 of 5

### Root cause and correction

Rollback restored all live files, then deleted every staged file and backup,
and only afterward removed the portable journal. A stop or journal-removal
failure in that window left a pending journal whose recovery sources no longer
existed. The transaction could therefore block every later portable-user load.

Rollback now removes and syncs the journal first. Only after that durable
rollback commit point does it cross the injectable cleanup boundary and delete
staging artifacts. If journal removal fails, every backup remains available for
the next recovery attempt. If the process stops after journal removal, the live
old generation is already complete and the next load safely removes the orphan
artifacts through the no-journal cleanup path.

The legacy migration helper documentation now matches its implementation: a
prior response address belongs to the named user only when it normalizes to an
allowlisted email; otherwise it remains unresolved. Brain was bumped from
`0.18.1` to `0.18.2`.

### RED evidence

The named test is
`crash_at_rollback_cleanup_boundary_keeps_recovery_possible`.

Its first RED run failed to compile because the wished-for
`TransactionStep::RollbackCleanup` injection boundary did not exist:

```text
error[E0599]: no variant or associated item named `RollbackCleanup` found for enum `TransactionStep`
```

After adding only the injection seam at the vulnerable old ordering, the test
reproduced the data-loss window. The injected crash left the journal present,
and the next `recover_pending` failed exactly on both deleted backups:

```text
called `Result::unwrap()` on an `Err` value: Transaction { message:
"portable user transaction failed: read user transaction backup at
.../tasks/.brain-user-...-0.backup: No such file or directory (os error 2);
portable user transaction failed: read user transaction backup at
.../.config/.brain-user-...-1.backup: No such file or directory (os error 2)" }
```

### GREEN evidence

- Exact named crash-boundary test: 1 passed.
- Isolated `cargo test --release users::store::transaction_tests`: 6 passed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- Scoped `rustfmt --check` for `transaction.rs`, `store.rs`, and `command.rs`:
  passed.
- `git diff --check`: passed.

### Files changed in this fix round

- `Cargo.toml`, `Cargo.lock`
- `src/users/{command,store,transaction}.rs`
- `docs/data-model.md`

Commit: the fix-round branch HEAD created immediately after this report; no push
or merge was performed.
