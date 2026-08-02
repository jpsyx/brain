# Brain Multi-Workspace Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan. Use rust-skills for every Rust task and test-driven-development for every behavior change.

**Goal:** Replace Brain's single implicit root with a machine-local workspace registry, a global `--brain/-b` selector, and an immutable workspace context that scopes all existing local and portable paths.

**Architecture:** Command entry resolves one workspace exactly once. It passes an immutable `WorkspaceContext` to stores, TUI setup, sessions, locks, and child-process builders. The only machine-global configuration is the registry at `~/.config/brain/env.json`; portable settings remain below each selected root.

**Tech Stack:** Rust 2024, clap, serde/serde_json, uuid, rusqlite, tempfile, existing themed `/dev/tty` prompting.

**Global Constraints:** Follow red/green TDD without exception. Preserve existing single-workspace behavior during migration. Do not use process-global mutable workspace state. Do not add a dependency. Preserve unrelated worktree changes. Update durable docs in the same implementation. Any implementation commit must bump `Cargo.toml` and `Cargo.lock` from the then-current version according to the repository policy.

**Depends on:** The approved design at `docs/superpowers/specs/2026-08-02-brain-multi-workspace-design.md`.

**Produces:** The workspace and path APIs consumed by all later multi-workspace plans.

---

### Task 1: Introduce validated workspace identity and derived paths

**Files:**

- Create: `src/workspace/mod.rs`
- Create: `src/workspace/id.rs`
- Create: `src/workspace/name.rs`
- Create: `src/workspace/context.rs`
- Create: `src/workspace/paths.rs`
- Modify: `src/lib.rs`

- [ ] **RED: add tests for names, immutable identity, and non-colliding paths**

Add unit tests beside the new modules. The core path test must be equivalent to:

```rust
#[test]
fn different_workspace_ids_never_share_runtime_paths() {
    let home = Path::new("/home/tester");
    let personal = WorkspacePaths::new(home, WorkspaceId::parse(PERSONAL_ID).unwrap());
    let family = WorkspacePaths::new(home, WorkspaceId::parse(FAMILY_ID).unwrap());

    assert_ne!(personal.cache_dir(), family.cache_dir());
    assert_ne!(personal.state_db(), family.state_db());
    assert_ne!(personal.tui_lock(), family.tui_lock());
    assert_ne!(personal.sync_dir(), family.sync_dir());
    assert_eq!(
        personal.cache_dir(),
        Path::new("/home/tester/.cache/brain/workspaces").join(PERSONAL_ID)
    );
}
```

Also prove that canonical names are trimmed, lower-case, limited to `[a-z0-9][a-z0-9_-]*`, and that `WorkspaceContext::root()` does not change when aliases or the machine default later change.

- [ ] Run `cargo test --release workspace::` and observe unresolved-module failures.

- [ ] **GREEN: add the smallest typed domain surface**

Implement and re-export this interface:

```rust
pub struct WorkspaceId(Uuid);
impl WorkspaceId {
    pub fn new() -> Self;
    pub fn parse(value: &str) -> Result<Self, WorkspaceIdError>;
}

pub struct WorkspaceName(String);
impl WorkspaceName {
    pub fn parse(value: &str) -> Result<Self, WorkspaceNameError>;
    pub fn from_root(root: &Path) -> Result<Self, WorkspaceNameError>;
    pub fn as_str(&self) -> &str;
}

pub struct WorkspaceContext {
    id: WorkspaceId,
    name: WorkspaceName,
    root: PathBuf,
    local_user_id: String,
    paths: WorkspacePaths,
}
impl WorkspaceContext {
    pub fn id(&self) -> WorkspaceId;
    pub fn name(&self) -> &WorkspaceName;
    pub fn root(&self) -> &Path;
    pub fn local_user_id(&self) -> &str;
    pub fn paths(&self) -> &WorkspacePaths;
}

pub struct WorkspacePaths { /* home plus stable workspace UUID */ }
```

`WorkspacePaths` must expose `cache_dir`, `state_db`, `tui_lock`, `inbox_dir`, `responses_dir`, `logs_dir`, and `sync_dir`. `cache_dir` borrows the stored base path; the child accessors return derived paths. Normalize the root to an absolute lexical path at construction, without requiring it to exist and without filesystem canonicalization.

- [ ] Run `cargo test --release workspace::` and keep it green.

- [ ] Refactor only names and module exports. Run `cargo clippy --release --all-targets`.

- [ ] Commit this task only if the selected execution workflow authorizes commits; include the required version bump.

### Task 2: Add the versioned machine registry and strict validation

**Files:**

- Create: `src/workspace/registry/mod.rs`
- Create: `src/workspace/registry/model.rs`
- Create: `src/workspace/registry/store.rs`
- Create: `src/workspace/registry/validate.rs`
- Create: `src/workspace/registry/select.rs`
- Modify: `src/workspace/mod.rs`

- [ ] **RED: test schema parsing and selection in pure functions**

Use JSON fixtures in inline tests to prove:

```rust
#[test]
fn alias_selects_one_canonical_workspace() {
    let registry = registry_with_brain_and_family();
    let selected = registry.select(Some("fam")).unwrap();
    assert_eq!(selected.canonical_name().as_str(), "family");
}

#[test]
fn omitted_selector_uses_default_without_changing_access_data() {
    let registry = registry_with_brain_and_family();
    let selected = registry.select(None).unwrap();
    assert_eq!(selected.canonical_name().as_str(), "brain");
    assert_eq!(selected.record().env.get("sentinel"), Some(&json!("personal")));
}

#[test]
fn duplicate_alias_and_overlapping_root_are_rejected() {
    let err = validate_registry(&invalid_registry()).unwrap_err();
    assert!(matches!(err, RegistryError::DuplicateSelector { .. }));
}
```

Add separate tests for an unknown selector, an alias collision under ASCII case folding, duplicate UUIDs, a missing default, exact duplicate roots, ancestor/descendant roots, and atomic rename preserving UUID and record contents.

- [ ] Run `cargo test --release workspace::registry::` and observe failures.

- [ ] **GREEN: implement schema version 2 and atomic persistence**

Implement these serializable types:

```rust
pub const REGISTRY_SCHEMA_VERSION: u32 = 2;

pub struct MachineRegistry {
    pub schema_version: u32,
    pub default_workspace: WorkspaceName,
    pub workspaces: BTreeMap<WorkspaceName, WorkspaceRecord>,
}

pub struct WorkspaceRecord {
    pub workspace_id: WorkspaceId,
    pub root: PathBuf,
    #[serde(default)]
    pub aliases: BTreeSet<WorkspaceName>,
    pub local_user_id: String,
    #[serde(default)]
    pub receiver_enabled: bool,
    #[serde(default)]
    pub env: serde_json::Map<String, serde_json::Value>,
}
```

Expose `MachineRegistry::select`, `create_record`, `attach_record`, `rename`, `add_alias`, `remove_alias`, `set_default`, and `remove`. Mutators validate a cloned candidate before `store::save_atomic` replaces the file. Removal must never touch the root.

`RegistryStore::load_from(path)` and `save_atomic_to(path, registry)` are the testable IO surface. Production `RegistryStore::real()` resolves `$XDG_CONFIG_HOME/brain/env.json` or `~/.config/brain/env.json`.

- [ ] Run the focused registry tests, then `cargo test --release`.

- [ ] Add a test that a failed validation leaves the original registry bytes unchanged, then implement the temporary-file plus rename write path.

- [ ] Commit this task only if authorized; include the required version bump.

### Task 3: Migrate the legacy flat env without losing data

**Files:**

- Create: `src/workspace/registry/migrate.rs`
- Create: `tests/workspace_registry_migration.rs`
- Modify: `src/workspace/registry/mod.rs`
- Modify: `src/env/migrate.rs`
- Modify: `src/env/store.rs`
- Modify: `src/paths.rs`

- [ ] **RED: add fixture-driven legacy migration tests**

Create temporary fixture directories in the integration test. Cover a flat `env.json` with `root`, agent commands, receiver values, and a nested `sync` block; a legacy `brain-root` pointer; and no prior files.

The main assertion must be equivalent to:

```rust
let outcome = migrate_legacy(&home, &config_dir, &legacy_body).unwrap();
let brain = outcome.registry.select(None).unwrap();
assert_eq!(brain.canonical_name().as_str(), "brain");
assert_eq!(brain.record().root, home.join("brain"));
assert_eq!(brain.record().env["sync"], legacy_json["sync"]);
assert_eq!(outcome.registry.default_workspace.as_str(), "brain");
assert!(outcome.backup_path.unwrap().exists());
```

Prove first-workspace access is not stored in the machine registry, because it belongs in portable config. Prove the migration is idempotent and never overwrites schema version 2.

- [ ] Run `cargo test --release --test workspace_registry_migration` and observe missing migration behavior.

- [ ] **GREEN: implement one-time migration**

Migration order:

1. Parse a valid schema version 2 registry and return it unchanged.
2. Otherwise read the flat env map and legacy root pointer using existing parsing helpers.
3. Derive the canonical name from the root basename, falling back to `brain`.
4. Generate one stable workspace UUID and write a backup beside `env.json` before replacement.
5. Move every flat machine-local key except `root` into the new record's `env` map.
6. Preserve the legacy pointer as read-only compatibility input, never as a new write target.

The migration result must report whether it created a registry, its backup path, and whether portable setup is still required.

- [ ] Run the integration test twice against the same fixture and verify byte-stable second-run output.

- [ ] Delete the old global env-map write path only after all existing env tests use a selected `WorkspaceRecord`.

- [ ] Commit this task only if authorized; include the required version bump.

### Task 4: Add global workspace selection and management commands

**Files:**

- Replace: `src/cli.rs` with `src/cli/mod.rs`
- Create: `src/cli/global.rs`
- Create: `src/cli/configuration.rs`
- Create: `src/cli/tasks.rs`
- Create: `src/cli/sync.rs`
- Create: `src/cli/server.rs`
- Create: `src/cli/workspace.rs`
- Create: `src/workspace/command/mod.rs`
- Create: `src/workspace/command/list.rs`
- Create: `src/workspace/command/mutate.rs`
- Create: `src/workspace/command/prompt.rs`
- Modify: `src/workspace/mod.rs`
- Modify: `src/main.rs`
- Create: `tests/workspace_cli.rs`

- [ ] **RED: test selector placement and the complete command grammar**

Add clap tests proving both positions select the same raw value:

```rust
for argv in [
    ["brain", "-b", "family", "sync"].as_slice(),
    ["brain", "sync", "--brain", "fam"].as_slice(),
] {
    let cli = Cli::try_parse_from(argv).unwrap();
    assert_eq!(cli.brain.as_deref(), Some(if argv.contains(&"fam") { "fam" } else { "family" }));
}
```

Add parsing tests for `workspace list/create/attach/rename/alias add/alias remove/default/remove`. Add a pure dispatch test proving `workspace remove` returns a registry mutation and never a filesystem-delete operation.

- [ ] Run `cargo test --release cli::` and `cargo test --release --test workspace_cli`; observe the new parsing/behavior failures.

- [ ] Before adding behavior, move the current clap types into the focused `src/cli/` modules, keep `Cli`, `Cmd`, and action types re-exported at their existing paths, and run the existing CLI suite green. `mod.rs` holds only parser entry, re-exports, and small shared glue.

- [ ] **GREEN: add `brain: Option<String>` as a global clap option**

Add:

```rust
#[arg(short = 'b', long = "brain", global = true, value_name = "WORKSPACE")]
pub brain: Option<String>;
```

Add the `Workspace` command tree from the approved design. Every value accepted interactively must also have a flag or positional non-interactive form. Use the existing themed `/dev/tty` helpers and extract shared prompting primitives instead of adding a prompt dependency.

- [ ] Make `workspace list` deterministic by canonical name and include canonical name, default marker, root, aliases, local user, receiver state, and portable access mode when available.

- [ ] Add integration tests with `XDG_CONFIG_HOME` and `HOME` isolated in `tempfile::TempDir`. Verify create, alias selection, rename, default change, attach, and non-destructive remove.

- [ ] Run `cargo test --release --test workspace_cli` and `cargo clippy --release --all-targets`.

- [ ] Commit this task only if authorized; include the required version bump.

### Task 5: Build one command bootstrap and readiness gate

**Files:**

- Create: `src/workspace/bootstrap.rs`
- Create: `src/workspace/readiness.rs`
- Create: `src/workspace/manifest.rs`
- Create: `src/command/mod.rs`
- Create: `src/command/dispatch.rs`
- Create: `src/command/configuration.rs`
- Create: `src/command/tasks.rs`
- Create: `src/command/sync.rs`
- Create: `src/command/server.rs`
- Create: `src/command/workspace.rs`
- Create: `src/command/reindex.rs`
- Modify: `src/main.rs`
- Modify: `src/workspace/mod.rs`
- Create: `tests/workspace_readiness.rs`

- [ ] **RED: characterize which invocations require a selected, ready workspace**

Define and test a pure classification:

```rust
assert_eq!(bootstrap_policy(&Invocation::Version), BootstrapPolicy::None);
assert_eq!(bootstrap_policy(&Invocation::AgentHook), BootstrapPolicy::InternalNoPrompt);
assert_eq!(bootstrap_policy(&Invocation::WorkspaceCreate), BootstrapPolicy::RegistryOnly);
assert_eq!(bootstrap_policy(&Invocation::WorkspaceList), BootstrapPolicy::ReadyWorkspace);
assert_eq!(bootstrap_policy(&Invocation::Sync), BootstrapPolicy::ReadyWorkspace);
assert_eq!(bootstrap_policy(&Invocation::Tui), BootstrapPolicy::ReadyWorkspace);
```

Add tests that an interactive missing manifest/local user returns `ReadinessAction::Prompt(fields)`, while a non-interactive command returns a typed error containing exact remediation commands. Help, version, hooks, and internal server invocations must not prompt.

Every ordinary user command, including workspace list/status commands, checks readiness. The only registry-only user commands are the setup and repair operations needed to create, attach, migrate, remove, or repair an incomplete workspace; those commands are themselves the guided setup path and must not be blocked by the missing values they repair.

- [ ] Run the focused tests and observe unresolved behavior.

- [ ] **GREEN: implement `bootstrap(cli, io) -> CommandContext`**

The successful command context is:

```rust
pub struct CommandContext {
    pub workspace: Arc<WorkspaceContext>,
    pub registry_store: RegistryStore,
}
```

`workspace.json` must contain `schema_version`, `workspace_id`, `receiver_ingress_id`, and minimum-compatible Brain version. Attaching validates that its UUID matches the machine record. Creating writes the manifest before registering the root, with cleanup limited to the newly created manifest if registry persistence fails.

- [ ] Move command handlers out of the oversized binary entry point before adding bootstrap branches. `src/main.rs` becomes parse, logging initialization, context-free early exits, bootstrap, dispatch, and top-level themed error rendering. Preserve behavior through the current CLI integration tests before changing dispatch.

Readiness initially verifies registry integrity, root/manifest UUID agreement, and a non-empty local user ID. The users plan extends it to verify registry membership and sender mappings.

- [ ] Refactor `main.rs` so command arms receive either no context, registry-only context, or one ready `WorkspaceContext`; remove ad hoc calls to `paths::brain_root()` from dispatch.

- [ ] Run all main/CLI/integration tests. Confirm an interactive repair continues the originally requested command rather than exiting after setup.

- [ ] Commit this task only if authorized; include the required version bump.

### Task 6: Thread workspace context through current stores and runtime paths

**Files:**

- Modify: `src/paths.rs`
- Modify: `src/env/store.rs`
- Modify: `src/env/vars.rs`
- Modify: `src/settings/store.rs`
- Modify: `src/settings/vars.rs`
- Modify: `src/personalization/store.rs`
- Modify: `src/personalization/runtime.rs`
- Modify: `src/state.rs`
- Modify: `src/session.rs`
- Modify: `src/tui/singleton.rs`
- Modify: `src/tui/event_loop/setup.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/reindex/mod.rs`
- Modify: `src/main.rs`

- [ ] **RED: add two-workspace characterization tests before changing signatures**

Test that two contexts in one process read different env/config/personalization files, open different state databases, acquire different TUI locks, and return different response directories. Also prove a second lock for the same UUID is rejected.

- [ ] Run the new tests and observe current global-path collisions.

- [ ] **GREEN: change store APIs to require context or explicit paths**

Target signatures:

```rust
env::resolve_one(workspace: &WorkspaceContext, name: &str) -> Option<String>
env::set(workspace: &WorkspaceContext, name: &str, value: &str) -> Result<()>
settings::load(workspace: &WorkspaceContext) -> Config
settings::set(workspace: &WorkspaceContext, name: &str, value: &str) -> Result<()>
personalization::load(workspace: &WorkspaceContext) -> Personalization
state::Db::open(workspace: &WorkspaceContext) -> Result<Db>
singleton::acquire(workspace: &WorkspaceContext) -> Result<Guard>
```

Remove `Personalization`'s process-wide `OnceLock`; cache it in the TUI `App` or reload from the selected path. `session::response_dir` becomes a `WorkspacePaths` method. Keep `paths::brain_root_path` only as a legacy migration helper, not a runtime source of truth.

- [ ] Thread `Arc<WorkspaceContext>` into `run_tui` and `App`. All existing root arguments should be derived from that context inside setup, not independently resolved.

- [ ] Add `WorkspaceContext::integration_env(actor_id)` returning only `BRAIN_WORKSPACE_ID`, `BRAIN_WORKSPACE`, `BRAIN_ROOT`, and `BRAIN_ACTOR_ID`. Use it for Brain-owned child scripts. Detached `brain` child commands must also carry `--brain <canonical-name>`.

- [ ] Run `rg -n 'brain_root\(|brain_root_path\(|default_path\(|response_dir\(' src` and classify every remaining occurrence as migration-only or a defect. Add a test for each allowed compatibility call site.

- [ ] Run `cargo test --release` and `cargo clippy --release --all-targets`.

- [ ] Commit this task only if authorized; include the required version bump.

### Task 7: Document and verify the foundation contract

**Files:**

- Modify: `README.md`
- Modify: `docs/glossary.md`
- Modify: `docs/architecture.md`
- Modify: `docs/features.md`
- Modify: `docs/data-model.md`
- Modify: `docs/config.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`

- [ ] Update docs with the registry schema, selector precedence, aliases, default semantics, manifest, cache layout, migration behavior, and explicit context flow.

- [ ] State that changing the default workspace never changes its access mode. Do not describe workspace-only mode as a filesystem sandbox.

- [ ] Add a docs guard test or extend the existing documentation assertions so CLI help and documented workspace commands agree.

- [ ] Run:

```sh
cargo test --release
cargo clippy --release --all-targets
./target/release/brain workspace list
./target/release/brain env list -b brain
./target/release/brain config list -b brain
```

Expected: all automated checks pass; each smoke command names or uses the selected workspace; the legacy installation appears as the default workspace without data loss.

- [ ] Inspect `git diff --check` and search modified prose for unsupported security claims and rhetorical em dashes.

- [ ] Commit this task only if authorized; include the required version bump.

## Foundation Exit Criteria

- Every user command resolves a workspace once or is explicitly classified as context-free.
- Existing users migrate to one unrestricted default workspace without losing env/config values.
- `-b` works before and after subcommands and resolves aliases.
- Portable and runtime paths do not collide across workspace UUIDs.
- No normal runtime module consults a global root.
- Later plans can accept `&WorkspaceContext` without reopening the registry.
