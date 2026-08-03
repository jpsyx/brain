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
