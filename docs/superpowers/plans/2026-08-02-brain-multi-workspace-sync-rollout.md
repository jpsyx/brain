# Brain Multi-Workspace Sync and Rollout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan. Use rust-skills, test-driven-development, and systematic-debugging for every sync or migration failure.

**Goal:** Scope every sync artifact and trigger to one workspace UUID, validate remote workspace identity, coordinate portable schema migration, and prove the full personal-plus-family acceptance scenario.

**Architecture:** `SyncRuntime` derives lock, journal, current-run, rclone workdir, CSV baselines, and freshness paths from `WorkspaceContext`. Sync configuration is loaded from only that workspace's machine registry record. A portable manifest is the local and remote identity/schema gate. Migration is journaled, backed up, resumable, and ordered so legacy CSV sync completes before UUID identity becomes authoritative.

**Tech Stack:** Rust 2024, rclone bisync/copyto, rusqlite, serde/serde_json, notify, csv, tempfile, existing local rclone integration harness.

**Global Constraints:** Execute after the other four plans, except that their migration seams may be built behind inactive gates. Preserve keep-both conflict behavior and max-delete protection. Different workspaces may sync concurrently; one workspace may not. Never sync `~/.config/brain/env.json`, credentials, cache, runtime sockets, or local-user selection. Never write a newer portable schema until readiness and compatibility checks pass. Any implementation commit must include the required version bump; the completed additive feature receives the appropriate pre-1.0 minor release bump from the then-current base.

---

### Task 1: Derive all sync runtime state from the workspace UUID

**Files:**

- Create: `src/sync/runtime.rs`
- Modify: `src/sync/mod.rs`
- Modify: `src/sync/lock.rs`
- Modify: `src/sync/journal.rs`
- Modify: `src/sync/current.rs`
- Modify: `src/sync/csv_sync.rs`
- Modify: `src/sync/run.rs`
- Modify: `src/sync/freshness.rs`
- Modify: `src/sync/follow.rs`
- Create: `tests/sync_workspace_paths.rs`

- [ ] **RED: test complete path separation**

```rust
let a = SyncRuntime::new(&personal_context());
let b = SyncRuntime::new(&family_context());

assert_ne!(a.lock_path(), b.lock_path());
assert_ne!(a.journal_path(), b.journal_path());
assert_ne!(a.current_path(), b.current_path());
assert_ne!(a.bisync_workdir(), b.bisync_workdir());
assert_ne!(a.csv_baseline("tasks.csv"), b.csv_baseline("tasks.csv"));
assert!(a.base_dir().ends_with(personal_id().to_string()));
```

Add a test proving two different workspace locks can be held concurrently, while a second lock for the same UUID is rejected. Test journal rows and current state cannot be read through another runtime.

- [ ] Run `cargo test --release --test sync_workspace_paths` and `cargo test --release sync::lock`; observe global collisions.

- [ ] **GREEN: implement `SyncRuntime` as the only path source**

```rust
pub struct SyncRuntime {
    workspace_id: WorkspaceId,
    root: PathBuf,
    sync_dir: PathBuf,
}
```

The sync dir is `workspace.paths.sync_dir()`. Pass explicit paths into `Journal::open`, `Reporter::begin`, lock acquisition, CSV baseline helpers, rclone workdir cleanup, status, and follow logic. Delete all sync `default_path`, `baseline_path`, and `bisync_workdir` functions that consult HOME.

- [ ] Search `src/sync` for `HOME`, `.cache/brain/sync`, `paths::brain_root`, and zero-argument path builders. Every remaining occurrence must be a fixture string or removed.

- [ ] Run all sync unit tests, integration tests that do not need rclone, and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 2: Scope sync configuration and remote identity to one workspace

**Files:**

- Modify: `src/sync/config.rs`
- Modify: `src/sync/remote.rs`
- Modify: `src/sync/setup.rs`
- Modify: `src/sync/check_access.rs`
- Modify: `src/sync/command/mod.rs`
- Modify: `src/workspace/manifest.rs`
- Create: `src/sync/identity.rs`
- Create: `tests/sync_workspace_identity.rs`

- [ ] **RED: test selected env and remote-manifest decisions**

Use two registry records with different buckets/keys and prove `SyncConfig::load(&family)` sees only family values. Add pure cases:

```rust
assert_eq!(
    check_remote_identity(local_id, None),
    RemoteIdentityDecision::Initialize
);
assert_eq!(
    check_remote_identity(local_id, Some(local_id)),
    RemoteIdentityDecision::Proceed
);
assert!(matches!(
    check_remote_identity(local_id, Some(other_id)),
    RemoteIdentityDecision::RefuseMismatch { .. }
));
```

- [ ] Run focused tests and observe global env behavior.

- [ ] **GREEN: make every sync entry point accept `&WorkspaceContext`**

`SyncConfig::load(workspace)` parses only `workspace.record.env.sync`. Remote builders take config plus manifest; no helper reopens global env.

- [ ] Before setup, repair, check, or sync writes anything, read remote `.config/workspace.json` through the configured rclone remote. An absent file is allowed only for empty/new setup. A different UUID is a hard refusal, including when two local names point to the same bucket/path.

- [ ] During initial setup, write the portable manifest locally first, upload it with the baseline, then read it back and verify UUID. Keep credentials in the machine record only.

- [ ] Add themed progress for local manifest validation, remote identity probe, lock acquisition, CSV merge, rclone phase, and journal write.

- [ ] Run identity/setup tests and full sync tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 3: Carry workspace selection through every automatic trigger

**Files:**

- Modify: `src/sync/trigger.rs`
- Modify: `src/sync/watch.rs`
- Modify: `src/sync/freshness.rs`
- Modify: `src/tui/app_sync.rs`
- Modify: `src/tui/event_loop/setup.rs`
- Modify: `src/server/receiver/dispatch.rs`
- Modify: `src/command/sync.rs`
- Create: `tests/sync_trigger_workspace.rs`

- [ ] **RED: test exact detached argv**

```rust
assert_eq!(
    detached_sync_args(&family_context(), Trigger::Watcher),
    ["sync", "--if-idle", "--brain", "family"]
);
assert_eq!(
    detached_sync_args(&personal_context(), Trigger::StartupPull),
    ["sync", "--pull", "--if-idle", "--brain", "brain"]
);
```

Prove alias input is canonicalized before child launch. Prove watcher callbacks capture immutable workspace UUID/name and cannot switch when the machine default changes.

- [ ] **RED: test concurrent workspaces and serialized same-workspace triggers**

Use injected runners. Start personal and family triggers simultaneously and observe both runners enter. Fire two family triggers and observe one coalesces/follows according to existing rules.

- [ ] Run focused tests and observe missing selector propagation/global lock behavior.

- [ ] **GREEN: make trigger constructors require context and runtime**

Startup pull, change-triggered push, manual sync, receiver freshness pull, watch debounce, follow, and status all carry the same workspace. Detached commands pass `--brain <canonical-name>` and `BRAIN_WORKSPACE_ID` as a defense-in-depth consistency check; command bootstrap refuses a mismatch.

- [ ] Each live TUI owns only its workspace watcher. Closing that TUI stops its watcher without affecting another workspace. Receiver freshness pulls complete before only that workspace's queued inbound job dispatch.

- [ ] Run trigger/watch/TUI/receiver tests and gated `tests/watch_local.rs` when its prerequisites are present.

- [ ] Commit only if authorized; include the required version bump.

### Task 4: Add portable schema compatibility and attach/join gates

**Files:**

- Modify: `src/workspace/manifest.rs`
- Modify: `src/workspace/readiness.rs`
- Modify: `src/workspace/command/mutate.rs`
- Create: `src/workspace/version.rs`
- Create: `src/workspace/compatibility.rs`
- Modify: `src/sync/setup.rs`
- Modify: `src/sync/command/mod.rs`
- Create: `tests/workspace_compatibility.rs`

- [ ] **RED: test compatibility decisions without filesystem IO**

```rust
assert_eq!(compat(client("0.16.0"), manifest(schema(2), min("0.16.0"))), Compatible);
assert!(matches!(compat(client("0.15.9"), manifest(schema(2), min("0.16.0"))), UpdateRequired { .. }));
assert!(matches!(compat(client("0.16.0"), manifest(schema(99), min("0.16.0"))), UnsupportedSchema { .. }));
```

Test malformed versions, older supported schemas, current schema, newer schema, root/manifest UUID mismatch, duplicate local UUID, and remote UUID mismatch.

- [ ] Run compatibility tests and observe missing behavior.

- [ ] **GREEN: implement a small internal semantic-version parser**

Support the crate's numeric `major.minor.patch` format without adding a dependency. Compare minimum client version and portable schema before any config/task write.

- [ ] `workspace attach <root>` reads the portable manifest, checks compatibility, refuses UUID conflicts, and then prompts/flags for machine-local root alias, local user ID, env, receiver state, and sync credentials. It never creates a new portable identity for an existing root.

- [ ] A newer manifest fails closed with exact update guidance in interactive and non-interactive modes. Hooks/internal server treat it as unavailable without prompting.

- [ ] Sync setup displays the local and remote workspace UUID/name and requires explicit confirmation before adopting a nonempty remote without a manifest. Non-interactive adoption requires a dedicated exact UUID flag; `--yes` alone is insufficient.

- [ ] Run compatibility, attach, and setup tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 5: Coordinate and journal the legacy-to-multi-workspace migration

**Files:**

- Create: `src/migration/mod.rs`
- Create: `src/migration/plan.rs`
- Create: `src/migration/journal.rs`
- Create: `src/migration/backup.rs`
- Create: `src/migration/steps.rs`
- Modify: `src/lib.rs`
- Modify: `src/workspace/bootstrap.rs`
- Modify: `src/workspace/readiness.rs`
- Modify: `src/workspace/manifest.rs`
- Modify: `src/workspace/registry/migrate.rs`
- Modify: `src/tasks/schema.rs`
- Modify: `src/sync/command/mod.rs`
- Create: `tests/multi_workspace_migration.rs`

- [ ] **RED: test the exact ordered plan**

```rust
assert_eq!(migration_plan(&legacy_fixture()), [
    Step::LegacySemanticSync,
    Step::BackupPortableData,
    Step::CreateWorkspaceManifest,
    Step::CreateUsersRegistry,
    Step::MigrateTaskColumnsAndUuids,
    Step::ReconcileManagedTriage,
    Step::RebuildDerivedData,
    Step::Verify,
    Step::MarkComplete,
]);
```

Add tests for no configured sync, incomplete required user mapping, interrupted step resumption, backup write failure, step failure before replacement, repeated completed migration, and current/newer schema no-op/refusal.

- [ ] Run migration tests and observe missing orchestrator.

- [ ] **GREEN: implement a resumable migration journal**

Machine-local migration state lives under the selected workspace cache at `migrations/<migration-id>.json` while active. Backups live under that cache's `migration-backups/<timestamp>-pre-multi-workspace/` and include config, personalization, users if present, both CSVs, counters, and schema metadata. They are never synced. Do not copy machine secrets into the backup.

- [ ] If sync is configured, run and journal one legacy semantic sync before adding UUID merge identity. Explain that every computer syncing this workspace must update Brain before continuing. Interactive mode requires confirmation; non-interactive mode requires `brain workspace migrate --brain <name> --acknowledge-all-machines-updated`.

- [ ] Each step writes new content to sibling temporary files, verifies it, atomically replaces targets, and records completion. Re-entry resumes after the last verified step. A failure leaves exact recovery instructions and the backup path.

- [ ] Ambiguous legacy sender mappings pause before portable mutation. Interactive setup maps each sender to an existing/new user; non-interactive migration lists exact `brain workspace user ...` commands and exits unchanged.

- [ ] Final verification checks manifest/registry UUID, user membership, task schema, UUID uniqueness, assignment membership, triage config consistency, derived indexes, and remote identity. Only then remove the active journal; retain backups.

- [ ] Run migration fixtures, full tests, and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 6: Audit optional versus required configuration per workspace

**Files:**

- Create: `src/workspace/requirements.rs`
- Modify: `src/workspace/readiness.rs`
- Modify: `src/config.rs`
- Modify: `src/env/schema.rs`
- Modify: `src/settings/schema.rs`
- Modify: `src/tasks/doctor.rs`
- Create: `tests/workspace_requirements.rs`

- [ ] **RED: encode a requirements matrix**

Test these minimums:

| Feature | Required | Optional when disabled |
| --- | --- | --- |
| Every workspace | manifest UUID, portable schema, at least one user, valid `local_user_id`, root | sync, SMS, email, MCPs, custom skills, triage habits |
| Sync enabled | bucket/path, credentials, remote UUID match | watcher |
| SMS enabled | provider secret/from/public URL, at least one allowed phone mapping | email fields |
| Email enabled | provider secret/domain/public URL, at least one allowed email mapping | phone fields |
| Workspace-only | access prompt, explicit capability policy | any MCP beyond the core set |
| Triage enabled | managed daily and weekly rows | modal display preference |

Add tests that disabling a feature removes its readiness errors, and that a configured but incomplete feature is reported as incomplete rather than silently disabled.

- [ ] Run requirements tests and observe scattered ad hoc validation.

- [ ] **GREEN: centralize `requirements(workspace) -> Vec<Requirement>`**

Use it for startup readiness, `brain workspace list`, `brain receiver status`, `brain sync status`, and `brain tasks doctor`. Each item has scope, status, interactive prompt metadata, and exact non-interactive remediation.

- [ ] Make the following explicitly optional per workspace: cloud sync, receiver as a whole, SMS, email, each MCP, each non-core skill, triage habit tracking, triage modal, PDF conversion, Linear integration, personalization role/org/tag styles, and browser/web views. The workspace itself, portable user registry, and local user selection remain required.

- [ ] Add themed doctor output grouped by selected workspace and feature. Do not print credentials or sender addresses unless a user explicitly requests config detail.

- [ ] Run requirements/readiness/doctor tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 7: Build the multi-workspace acceptance harness

**Files:**

- Create: `tests/multi_workspace_acceptance.rs`
- Create: `tests/fixtures/multi_workspace/personal/`
- Create: `tests/fixtures/multi_workspace/family/`
- Modify: `tests/sync_local.rs`
- Modify: `tests/watch_local.rs`

- [ ] **RED: add one hermetic acceptance scenario using fake agent/provider transports**

The test must prove:

1. Personal and family roots register with different UUIDs and caches.
2. Personal defaults unrestricted; family is workspace-only.
3. `-b fam` selects family while omitted selector selects personal.
4. Two TUIs acquire different locks and register with one shared server.
5. A family inbound SMS resolves to `wife` and creates a task assigned to `wife`.
6. Personal and family sync locks can be held together.
7. Independent same-display-ID tasks merge and deterministically renumber.
8. Family triage disabled state contains no managed triage history or modal.
9. Family launch includes the advisory root prompt and no personal capability credentials.
10. Closing family yields unavailable family receiver behavior while personal remains active.
11. Closing personal terminates the shared server.

- [ ] Run the test and watch it fail at the first missing integration seam. Turn it green one assertion at a time, returning to the owning plan/task for production changes rather than adding acceptance-only branches.

- [ ] Add a gated real-rclone variant with two local remotes and two workspace UUIDs. Prove concurrent workspaces do not share workdirs/baselines and a remote UUID mismatch refuses before bisync.

- [ ] Run acceptance, local sync when available, and full tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 8: Update all durable docs, release surfaces, and final verification

**Files:**

- Modify: `README.md`
- Modify: `docs/README.md`
- Modify: `docs/glossary.md`
- Modify: `docs/architecture.md`
- Modify: `docs/features.md`
- Modify: `docs/data-model.md`
- Modify: `docs/config.md`
- Modify: `docs/integrations.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `docs/keybindings.md`
- Modify: `AGENTS.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] Reconcile every documentation contract row with the finished module tree and CLI. Remove single-root, machine-wide sync lock, always-on server, global receiver, Claude-specific session schema, and `~/brain`-only language where it is no longer true.

- [ ] Keep the public security warning prominent and exact: workspace-only is advisory prompt enforcement plus best-effort capability filtering, easy to bypass, and not tenant isolation.

- [ ] Update AGENTS orientation, build smoke commands, docs-contract module paths, and frontend parity rule to name `WorkspaceContext`, `ActorContext`, `AgentController`, OpenCode's stub status, and the shared TUI-lifetime server.

- [ ] Verify CLI help and examples for `--brain/-b`, workspace commands, receiver/server removals, `--open-code/-oc`, and conflicting frontend flags.

- [ ] Apply the final additive pre-1.0 minor version bump from the implementation branch's current version and regenerate `Cargo.lock`. If prior authorized implementation commits already bumped versions, choose the next semantically valid minor rather than lowering the version.

- [ ] Run the complete release gate:

```sh
cargo test --release
cargo clippy --release --all-targets
cargo test --release bundled_skills_carry_no_personal_data
python3 -m unittest discover -s skills/todo/scripts/tests
```

Also run gated local rclone/watch tests when prerequisites exist, migration fixtures from legacy through current, and the multi-workspace acceptance harness.

- [ ] Run smoke commands against temporary HOME/XDG directories, never the real roots:

```sh
brain workspace list
brain config list -b brain
brain env list -b family
brain sync status -b family
brain receiver status -b family
brain server status
brain --open-code -b family
```

- [ ] Run `git diff --check` and targeted searches for stale global paths, obsolete commands, hard-coded roots, frontend branches, security overclaims, unfinished markers, and rhetorical em dashes.

- [ ] If the user authorizes the public brain-core publication workflow, commit and push the verified final change to `main` as allowed by AGENTS. Do not publish before every required check passes.

## Final Exit Criteria

- The full approved acceptance scenario passes in one hermetic test and in documented manual smoke checks.
- Every portable and runtime write is selected by immutable workspace context.
- Sync identity, locks, workdirs, baselines, journals, watchers, and freshness state are UUID-scoped.
- Legacy migration is backed up, resumable, idempotent, and ordered before UUID merge cutover.
- Optional features do not create unrelated setup requirements.
- Public docs accurately describe both multi-workspace behavior and the limits of prompt-based protection.
