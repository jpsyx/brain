# Brain Workspace Users, Tasks, and Triage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan. Use rust-skills for Rust work, test-driven-development for every behavior, and systematic-debugging before changing code in response to a failing existing test.

**Goal:** Add portable workspace users and sender mappings, preserve actor identity through every prompt, assign tasks to the effective actor, give task rows immutable merge identity, and make managed triage habits an optional workspace feature.

**Architecture:** `users.json` is the synced identity registry. A request resolves one immutable `ActorContext` before reaching task or agent code. CSV records gain `task_uuid`, `assigned_to`, and `system_key`; Rust and bundled Python mutators share the same environment contract. Merge identity is UUID-based, while `T###` and `H###` remain human-facing IDs that may be deterministically reconciled.

**Tech Stack:** Rust 2024, serde/serde_json, uuid v4 plus uuid v5, csv, existing bundled Python scripts, tempfile.

**Global Constraints:** Complete the foundation plan first. Follow strict red/green TDD. Do not add audit, owner, or authentication concepts. Do not retain the legacy `assignee` column after migration. Disabling triage removes definitions, open occurrences, completed history, and derived references. Manual triage remains available. Any implementation commit must include the repository-required version bump.

---

### Task 1: Add the portable user registry and normalized contact identities

**Files:**

- Create: `src/users/mod.rs`
- Create: `src/users/id.rs`
- Create: `src/users/model.rs`
- Create: `src/users/normalize.rs`
- Create: `src/users/store.rs`
- Create: `src/users/validate.rs`
- Create: `src/users/command.rs`
- Modify: `src/lib.rs`
- Create: `src/cli/users.rs`
- Create: `src/command/users.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/command/dispatch.rs`
- Modify: `src/workspace/context.rs`
- Modify: `src/workspace/registry/model.rs`
- Modify: `src/workspace/readiness.rs`
- Create: `tests/users_store.rs`

- [ ] **RED: test portable parsing, normalization, and uniqueness**

Add tests equivalent to:

```rust
#[test]
fn phone_and_email_resolve_to_one_portable_user() {
    let users = Users::parse(FIXTURE).unwrap();
    assert_eq!(users.resolve_phone("(212) 555-0100").unwrap().id.as_str(), "pablo");
    assert_eq!(users.resolve_email(" Wife@Example.COM ").unwrap().id.as_str(), "wife");
}

#[test]
fn one_enabled_sender_cannot_identify_two_users() {
    let err = Users::parse(DUPLICATE_PHONE_FIXTURE).unwrap_err();
    assert!(matches!(err, UsersError::DuplicateInboundPhone { .. }));
}
```

Cover lowercase kebab user IDs, E.164 output, ASCII-lowercase trimmed emails, multiple phone/email values for one user, disabled inbound addresses, duplicate enabled addresses, and byte-stable round trips.

- [ ] Run `cargo test --release users::` and `cargo test --release --test users_store`; observe missing modules.

- [ ] **GREEN: implement the portable schema**

```rust
pub struct UserId(String);

pub struct Users {
    pub schema_version: u32,
    pub users: Vec<User>,
}

pub struct User {
    pub id: UserId,
    pub name: String,
    #[serde(default)]
    pub phones: Vec<PhoneIdentity>,
    #[serde(default)]
    pub emails: Vec<EmailIdentity>,
    #[serde(default)]
    pub response_email: Option<String>,
}

pub struct PhoneIdentity {
    pub value: String,
    #[serde(default)]
    pub inbound_allowed: bool,
}

pub struct EmailIdentity {
    pub value: String,
    #[serde(default)]
    pub inbound_allowed: bool,
}
```

Store at `workspace.root/.config/users.json` through `UsersStore::load(&WorkspaceContext)` and atomic `save`. Phone normalization accepts existing common North American formatting only when it can produce an unambiguous E.164 value; otherwise setup requires the explicit country-prefixed form. Email normalization trims and ASCII-lowercases, without provider-specific dot or plus rewriting.

`response_email`, when present, must equal one normalized email on the same user. Migrate the old workspace-level response email to the uniquely matching user; ambiguous values enter the readiness prompt and are never guessed.

- [ ] Add selected-workspace CLI management with complete non-interactive forms:

```text
brain user list
brain user add --id <id> --name <name> [--phone <e164>] [--email <address>] [--response-email <address>]
brain user update <id> [--name <name>] [--add-phone <e164>] [--add-email <address>] [--response-email <address>]
brain user remove <id> [--reassign-to <id>]
brain user local <id>
```

Omitted values use themed interactive prompts. Removing a user refuses while tasks remain assigned to that ID unless `--reassign-to` names another member; it updates task assignment and then removes the user atomically. Changing `local` edits only the selected machine registry record.

- [ ] Extend readiness so `local_user_id` must name a user in this registry. A missing or invalid mapping is promptable interactively and an exact typed error non-interactively.

On first setup, ask the user's display name, propose a normalized user ID, create that portable user, and set the machine's `local_user_id`. Do not ask for phone or email unless its receiver channel is configured.

- [ ] Add a migration helper that converts the prior personalization name into the first user ID, proposing a normalized ID but allowing an interactive override. It must not invent names for ambiguous legacy allowlist entries.

- [ ] Run focused tests, full tests, and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 2: Resolve and preserve the effective actor

**Files:**

- Create: `src/actor/mod.rs`
- Create: `src/actor/context.rs`
- Create: `src/actor/resolve.rs`
- Modify: `src/lib.rs`
- Modify: `src/workspace/context.rs`
- Modify: `src/server/security.rs`
- Modify: `src/server/receiver/http/sms.rs`
- Modify: `src/server/receiver/http/email.rs`
- Modify: `src/server/receiver.rs`
- Modify: `src/state.rs`
- Create: `tests/actor_resolution.rs`

- [ ] **RED: test precedence and rejection as pure decisions**

```rust
#[test]
fn inbound_sender_overrides_the_machine_local_user() {
    let actor = resolve_actor(
        UserId::parse("pablo").unwrap(),
        RequestIdentity::Sms { from: "+12125550101" },
        &family_users(),
    ).unwrap();
    assert_eq!(actor.user_id().as_str(), "wife");
    assert_eq!(actor.channel(), Channel::Sms);
}

#[test]
fn terminal_request_uses_local_user() {
    let actor = resolve_actor(local("pablo"), RequestIdentity::Local, &users()).unwrap();
    assert_eq!(actor.user_id().as_str(), "pablo");
}

#[test]
fn unknown_inbound_sender_is_rejected() {
    assert!(matches!(
        resolve_actor(local("pablo"), unknown_sms(), &users()),
        Err(ActorError::UnknownOrDisallowedSender)
    ));
}
```

Also test email precedence, disabled sender rejection, and a follow-up turn retaining the initiating actor even when the machine local user differs.

- [ ] Run the focused tests and observe failures.

- [ ] **GREEN: implement immutable actor context**

```rust
pub struct ActorContext {
    pub user_id: UserId,
    pub display_name: String,
    pub channel: Channel,
}

pub enum Channel { Interactive, Sms, Email }
pub enum RequestIdentity<'a> {
    Local,
    Sms { from: &'a str },
    Email { from: &'a str },
}
```

The receiver must authenticate the provider request first, then resolve the sender, then create a queued in-memory job containing workspace UUID plus `ActorContext`. Do not put an untrusted sender string into `BRAIN_ACTOR_ID`.

- [ ] Generalize the state database columns from Claude-specific session identity to `agent_kind`, `agent_session_id`, `workspace_id`, `actor_id`, and `channel`. Write a schema migration that preserves current rows as the local user's interactive Claude rows.

- [ ] Ensure every agent follow-up, completion hook, task mutation, and response delivery reads actor identity from the job/session context, never from current machine defaults.

- [ ] Run actor, receiver-security, and state migration tests; then full tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 3: Add `assigned_to` and make bundled mutators workspace-aware

**Files:**

- Modify: `src/tasks/task/mod.rs`
- Modify: `src/tasks/task/load.rs`
- Modify: `src/tasks/complete.rs`
- Modify: `src/tasks/revive.rs`
- Modify: `src/tasks/skip.rs`
- Modify: `src/reindex/tasks.rs`
- Modify: `skills/todo/SKILL.md`
- Modify: `skills/todo/references/commands.md`
- Modify: all `skills/todo/scripts/*.py` files that resolve Brain paths or create rows
- Modify: affected `skills/triage` scripts and instructions
- Create: `skills/todo/scripts/tests/test_workspace_context.py`

- [ ] **RED: add Rust reader compatibility tests**

Prove readers accept legacy `assignee`, prefer `assigned_to` when both exist, and expose the normalized field on `Task`:

```rust
assert_eq!(load_one("...,assignee,...\n...,pablo,...").assigned_to, "pablo");
assert_eq!(
    load_one("...,assignee,assigned_to,...\n...,legacy,wife,...").assigned_to,
    "wife"
);
```

Add a pure creation-default test: one user and multiple users both default to `actor.user_id`; editing any unrelated field preserves assignment; explicit reassignment validates membership.

Add `assignment_ui_mode(users)` tests: one user hides assignment controls and filters while still auto-filling the ID; multiple users show assignment in task detail, creation/reassignment controls, and an assignee filter without changing the default-to-actor rule.

- [ ] Run the focused Rust tests and observe failures.

- [ ] **RED: add Python subprocess tests with isolated environment**

Run `add_task.py` in a temporary workspace with:

```text
BRAIN_ROOT=<temp root>
BRAIN_WORKSPACE=family
BRAIN_WORKSPACE_ID=<fixture uuid>
BRAIN_ACTOR_ID=wife
```

Assert the script touches only the temporary root and writes `assigned_to=wife`. Clear `BRAIN_ROOT` and assert a direct, nonzero error that tells callers to launch the script through Brain. Search every bundled script for `Path.home() / "brain"` and add a guard test that rejects it.

- [ ] **GREEN: centralize Python context in `_csvlib.py`**

Expose `brain_root()`, `actor_id()`, `tasks_csv()`, `habits_csv()`, and UUID creation through one helper. All mutators import these helpers. Replace the `--assignee` option with `--assigned-to`; allow explicit reassignment only after validating the ID through portable `users.json`. Omission uses `BRAIN_ACTOR_ID`.

- [ ] Update Rust task commands to accept `&WorkspaceContext` and `&ActorContext`, removing all internal calls to `paths::brain_root()`.

- [ ] Migrate headers from `assignee` to `assigned_to` by column name, preserve values, and make all future writers emit only `assigned_to`.

- [ ] Run Rust tests plus `python3 -m unittest discover -s skills/todo/scripts/tests`.

- [ ] Commit only if authorized; include the required version bump.

### Task 4: Introduce immutable task UUIDs with deterministic legacy migration

**Files:**

- Create: `src/tasks/identity.rs`
- Create: `src/tasks/schema.rs`
- Modify: `src/tasks/task/mod.rs`
- Modify: `src/tasks/task/load.rs`
- Modify: `src/tasks/complete.rs`
- Modify: `skills/todo/scripts/_csvlib.py`
- Modify: `skills/todo/scripts/add_task.py`
- Modify: `skills/todo/scripts/next_habit_occurrence.py`
- Create: `tests/task_schema_migration.rs`

- [ ] **RED: test deterministic legacy identity**

Enable UUID v5 in the existing `uuid` dependency, then add tests equivalent to:

```rust
let first = legacy_task_uuid(workspace_id, CsvKind::Tasks, "T42");
let second = legacy_task_uuid(workspace_id, CsvKind::Tasks, "T42");
assert_eq!(first, second);
assert_ne!(first, legacy_task_uuid(other_workspace_id, CsvKind::Tasks, "T42"));
assert_ne!(first, legacy_task_uuid(workspace_id, CsvKind::Habits, "T42"));
```

Add fixture tests proving migration keeps every row, preserves display IDs, backs up both CSVs, is idempotent, and gives a spawned habit occurrence a new UUID while retaining its `system_key` and assignment.

- [ ] Run the migration integration test and observe missing columns.

- [ ] **GREEN: add `task_uuid` as the first immutable merge key**

Use UUID v5 namespace input `<workspace-uuid>:<csv-kind>:<legacy-task-id>` for migrated rows. Use UUID v4 for all new rows. Neither edit, completion, recurrence, nor display-ID reconciliation may alter an existing row's UUID.

The schema migration order is:

1. Finish one legacy semantic sync through the rollout plan's migration gate.
2. Back up `tasks.csv`, `habits.csv`, counter files, and schema metadata.
3. Align legacy columns by name.
4. Add deterministic UUIDs and `assigned_to`.
5. Record the portable task schema version.
6. Atomically replace each CSV.

- [ ] Add mutation tests proving completion and edits locate by display ID for user commands but update the UUID-bearing row and preserve its UUID.

- [ ] Update the task schema documentation generator/source so `task_uuid` is the merge key and `task_id` is explicitly mutable display identity.

- [ ] Run full Rust and Python tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 5: Merge CSVs by UUID and deterministically reconcile display-ID collisions

**Files:**

- Replace: `src/sync/csv_merge.rs` with `src/sync/csv_merge/mod.rs`
- Create: `src/sync/csv_merge/table.rs`
- Create: `src/sync/csv_merge/merge.rs`
- Create: `src/sync/csv_merge/reconcile.rs`
- Create: `src/sync/csv_merge/relationships.rs`
- Modify: `src/sync/csv_sync.rs`
- Modify: `src/sync/check.rs`
- Modify: `src/sync/counters.rs`
- Create: `tests/task_id_collision_merge.rs`

- [ ] **RED: characterize current three-way merge behavior before refactoring**

Move existing tests without changing assertions. Add a test proving headers with the same names in different orders merge correctly by name, not index.

- [ ] Run `cargo test --release sync::csv_merge::` and keep characterization green before structural edits.

- [ ] **RED: add the independent-create collision fixture**

Base has highest ID `T9`. Local and remote independently contain distinct UUID rows both displayed as `T10`; each has a dependent task whose `blocked_by` points to its own parent's UUID through the relationship resolution model. Expected merge:

```rust
assert_eq!(merged.rows_by_uuid().len(), 4);
assert_eq!(display_id(local_uuid), "T10");
assert_eq!(display_id(remote_uuid), "T11");
assert_eq!(display_id(remote_child_uuid), "T12");
assert_eq!(blocked_by(remote_child_uuid), [remote_uuid]);
assert_eq!(reconcile(merged.clone()), reconcile(reconcile(merged)));
```

The stable winner is the lexicographically smaller UUID. Allocate replacement IDs after the maximum numeric display ID across base, local, and remote. Add mirror-order tests proving swapping local and remote gives identical output.

- [ ] **GREEN: implement name-aligned tables keyed by `task_uuid`**

Parse headers into a `ColumnMap`. Require supported schema version and required columns. Preserve unknown forward-compatible columns only when the manifest declares compatibility; otherwise refuse before writing. Keep existing three-way field conflict rules, but key rows by UUID.

- [ ] Implement relationship rewriting in one pure function. Resolve display-ID references against the pre-reconciliation side-specific map, then emit the final display IDs. Cover `blocked_by`, chunk chains, project metadata task lists, and any other repository search result that stores `T###`/`H###` relationships.

- [ ] Advance counter files beyond every emitted display ID and prove repeated sync cannot issue a duplicate.

- [ ] Run collision tests, all sync tests, and full tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 6: Add managed triage habits and complete disable purge

**Files:**

- Modify: `src/config.rs`
- Modify: `src/settings/schema.rs`
- Modify: `src/settings/vars.rs`
- Create: `src/tasks/triage_habits/mod.rs`
- Create: `src/tasks/triage_habits/model.rs`
- Create: `src/tasks/triage_habits/reconcile.rs`
- Create: `src/tasks/triage_habits/purge.rs`
- Modify: `src/tasks/complete.rs`
- Modify: `src/tasks/revive.rs`
- Modify: `src/tasks/skip.rs`
- Modify: `src/tui/app_actions/triage.rs`
- Modify: `src/tui/app_state/construct.rs`
- Modify: `src/reindex/tasks.rs`
- Modify: `skills/todo/scripts/cleanup_done_habits.py`
- Modify: `skills/todo/scripts/apply_sync_rules.py`
- Modify: `skills/todo/SKILL.md`
- Modify: `skills/triage/SKILL.md`
- Create: `tests/triage_habits_config.rs`

- [ ] **RED: test the config default and modal suppression**

```rust
assert!(Config::default().enable_triage_habits);
assert!(triage_modal_target(false, false, &habits, pattern, today).is_none());
assert!(triage_modal_target(true, false, &habits, pattern, today).is_some());
```

The first boolean is `enable_triage_habits`; the second remains the process-scoped modal skip. A false feature flag wins over all modal preferences.

- [ ] **RED: test managed-definition protection**

Use stable `system_key` values `brain.triage.daily` and `brain.triage.weekly`. Prove reconciliation creates exactly one current definition/occurrence of each, completion carries the marker to the next occurrence, rename does not affect identity, and user remove returns `ManagedTaskCannotDelete` while enabled.

- [ ] **RED: test the complete purge set**

Build a fixture containing managed definitions, open rows, completed history, derived agenda/index entries, similarly named unmarked user habits, and unrelated transcripts. After disabling:

```rust
assert!(all_csv_rows.iter().all(|row| !row.is_managed_triage()));
assert!(!derived_text.contains(managed_daily_uuid));
assert!(rows_named_triage_but_unmarked_are_preserved());
assert!(unrelated_transcript.exists());
```

Prove re-enabling creates fresh UUIDs and no history.

- [ ] Run the focused tests and observe failures.

- [ ] **GREEN: implement one transactional reconciler**

`apply_triage_habits_config(workspace, enabled)` owns creation and purge. On `true`, it ensures the managed daily and weekly chains exist. On `false`, it removes all rows carrying either system key and rewrites derived task/agenda indexes before saving portable config. If any filesystem write fails, the config value and CSVs remain at their prior state.

- [ ] Make normal task/habit removal call `can_remove(row, config)`. Garbage collection may remove old completed managed occurrences only under the existing retention rule while the feature is enabled.

- [ ] Ensure manual `/triage` instructions do not require managed habits. When the feature is off, completion steps skip habit mutation and still perform cleanup work.

- [ ] Run the full Rust/Python tests, and review the diff by hand for personal
      data in any bundled skill (there is no automated guard).

- [ ] Commit only if authorized; include the required version bump.

### Task 7: Update data, config, feature, and testing documentation

**Files:**

- Modify: `README.md`
- Modify: `docs/features.md`
- Modify: `docs/data-model.md`
- Modify: `docs/config.md`
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`

- [ ] Document `users.json`, local versus inbound identity, `assigned_to`, immutable UUIDs, display-ID reconciliation, the temporary `assignee` reader, and the triage purge semantics.

- [ ] State explicitly that there is no owner, creator, audit history, or cross-machine distinction for the same user ID.

- [ ] Add migration fixtures to the documented test matrix, including two-machine same-ID creation, relationship rewrites, and re-enable-after-purge.

- [ ] Run:

```sh
cargo test --release
cargo clippy --release --all-targets
python3 -m unittest discover -s skills/todo/scripts/tests
```

Expected: all checks pass; no bundled task script contains a hard-coded `~/brain`; the family fixture assigns an inbound wife's task to her portable user ID.

- [ ] Inspect `git diff --check`, check schema examples against actual serialization, and review all deletion tests for false-positive data loss.

- [ ] Commit only if authorized; include the required version bump.

## Users, Tasks, and Triage Exit Criteria

- Every request has one immutable effective actor before agent or task work begins.
- Inbound sender identity overrides `local_user_id`; unknown senders are rejected.
- New tasks default to the effective actor and edits preserve assignment.
- UUID-distinct tasks survive display-ID collisions and reconcile identically on every machine.
- Turning triage habits off removes every managed definition, open occurrence, completed occurrence, and derived reference, while preserving unmarked user data.
- Bundled task and triage tooling is portable across roots and cannot silently fall back to `~/brain`.
