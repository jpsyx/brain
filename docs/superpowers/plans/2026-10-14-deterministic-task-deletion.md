# Deterministic Task Deletion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every supported task deletion execute a declared native policy, maintain relationships, and upgrade installed managed skills without invoking an agent frontend.

**Architecture:** A pure removal planner produces a fingerprinted batch; one workspace-scoped executor coordinates local transactions, optional external adapters, and derived agenda refresh. CLI, TUI, and maintenance callers share that service. A versioned skills-install coordinator serves installers, startup reconciliation, and explicit repair.

**Tech Stack:** Existing Rust 2024, clap, serde/serde_json, chrono, sha2, uuid, SQLite task locks, ratatui/crossterm, tempfile, and the existing grouped filesystem transaction implementation. No new production dependency is required for core.

**Spec:** [Deterministic task deletion](../specs/2026-10-14-deterministic-task-deletion-design.md).

**Baseline:** `fe477ec`, version `0.101.0`. Reconcile paths with the execution checkout before starting; do not overwrite intervening work.

**Status:** Core implementation and acceptance complete. Required release checks and canonical, fake-adapter, and upgrade/downgrade acceptance passed. Integration deployment is a separate workspace-owned step. The checklist below records the approved plan; Task 20 records completed core acceptance.

---

## Global constraints

- Follow repository `AGENTS.md` and applicable workspace rules. Use the required worktree tool. This plan does not authorize a merge, publication, or live deletion beyond existing session permissions.
- Write a failing behavioral test, observe its failure, implement the smallest passing change, then run the focused suite. A compile failure for a genuinely missing API is a valid first red; subsequently require behavioral reds.
- Keep every new module focused and below approximately 400 production lines. Split tests into sibling modules when needed.
- Use synthetic workspaces and fake external adapters. Never test against the user's task CSVs, credentials, tracker, or installed skills.
- All public examples use generic names. Do not copy private extension content into core.
- Keep CLI progress on stderr; JSON mode produces exactly one object on stdout. Typed domain errors must survive the CLI/TUI boundary.
- Every commit follows the repository's version-bump rule. Bump `Cargo.toml` and the brain package entry in `Cargo.lock` together. Use the next available minor for the feature's first committed change, then patches for subsequent compatible commits; do not pin a stale release number in the migration table.
- Commit commands in this plan are execution checkpoints, not instructions to publish. At each checkpoint stage only the task's listed files, matching docs, and the required version files.

## Delivery order and coverage

| Stage | Tasks | Spec coverage | Independently verifiable result |
|---|---|---|---|
| Local semantics | 1-8 | R1-R5, R14 | Pure plans, relationship repair, durable grouped writes |
| Native operation | 9-14 | R3, R5-R10 | CLI, integrations, TUI, maintenance |
| Instructions and installation | 15-19 | R11-R13 | Correct skill commands and automatic compatibility refresh |
| Acceptance | 20-21 | All | Regression proof and provider deployment contract |

Do tasks sequentially through their dependency points. In particular, do not
wire the TUI to the old row-only primitive while the relationship engine is
unfinished. Provider deployment in Task 21 is separately owned and may proceed
after Task 10, but it is required before claiming an existing tracker-enabled
workspace is fully deterministic.

## File map

| Path | Change and responsibility |
|---|---|
| `src/tasks/removal/mod.rs` | New public-to-crate service facade and module declarations |
| `src/tasks/removal/model.rs` | Serializable options, identities, effects, errors, plans, receipts |
| `src/tasks/removal/snapshot.rs`, `resolve.rs` | Read coherent workspace inputs and resolve explicit identities |
| `src/tasks/removal/dependencies.rs`, `references.rs`, `projects.rs` | Pure relationship transformations |
| `src/tasks/chunk_family.rs` | Shared family and next-surviving-chunk selection |
| `src/tasks/removal/plan.rs`, `fingerprint.rs` | Compose exact effects and canonical state digest |
| `src/tasks/removal/execute.rs`, `operations.rs`, `src/workspace/paths.rs` | Consent, transaction orchestration, durable operation/recovery states and cache paths |
| `src/tasks/removal/integration/{mod,config,protocol,process}.rs` | Narrow optional external cancellation boundary |
| `src/tasks/removal/tests/{mod,fixture,planning,execution,integration}.rs` | Behavioral fixtures and pure/service tests |
| `src/tasks/store_transaction/` | Extract shared grouped filesystem transaction; caller-specific target policy |
| `src/tasks/triage_habits/transaction/` | Compatibility facade preserving existing recovery behavior |
| `src/tasks/mutate/remove.rs` | Delegate existing removal callers to the shared engine |
| `src/tasks/cli.rs`, `src/command/tasks/mutate/mod.rs` | CLI grammar and dispatch |
| `src/command/tasks/mutate/remove/{mod,interactive,report}.rs` | Human/JSON shell around the shared service |
| `src/tasks/agenda/{sync,mod}.rs`, `src/tasks/complete/complete_ops.rs` | Removal refresh and gap-aware chunk handoff |
| `src/command/backlog.rs`, `src/tasks/backlog/{purge,dedupe}.rs` | Shared cleanup with existing maintenance policies |
| `src/command/project.rs`, `src/sync/csv_sync/{mod,metadata}.rs`, `src/reindex/` | Participate in task/project writer coordination |
| `src/tui/task_removal/{mod,state,worker,render}.rs` | Removal modal and bounded native execution |
| `src/tui/app_actions/{commands,task_commands}.rs` | Replace deletion prompt with plan-backed action |
| `src/tui/{modal_state,draw_modals}.rs`, `src/tui/overlay/mod.rs`, `src/tui/handlers/overlay.rs`, `src/tui/runtime/tick.rs` | Modal lifecycle and completion polling |
| `src/{config.rs,settings/schema.rs,settings/vars.rs,env/schema.rs}` | Portable integration policy and machine adapter settings |
| `skills/{todo,triage,second-brain}/` | Bundled command contract and references |
| `src/skills/{install,mod,command}.rs`, new `src/skills/{reconcile,install_transaction}.rs` | Safe installation, per-workspace reconciliation, status |
| `src/cli/configuration.rs`, `src/command/configuration.rs` | Explicit all-workspace skill repair |
| `src/startup_migration/task_deletion_skills.rs`, `src/startup_migration/mod.rs`, `src/cli/internal.rs`, `src/main.rs`, `install.sh` | Automatic upgrade/downgrade orchestration |
| `tests/task_removal_cli.rs`, `tests/task_removal_sync.rs`, `tests/skill_command_examples.rs`, `tests/task_deletion_skill_migration.rs`, `tests/install_script.rs` | Public acceptance tests |
| `docs/{features,architecture,data-model,integrations,config,keybindings,testing,decisions}.md` | Product behavior and rationale |

## Task 1: Define removal options, typed errors, and explicit target identity

**Files:** Create `src/tasks/removal/{mod,model,resolve}.rs` and `src/tasks/removal/tests/{mod,fixture,planning}.rs`; modify `src/tasks/mod.rs`.

- [ ] **Write the first failing test** in `resolve.rs`. Reuse the existing `CsvFile`, `Row`, `locate`, and `legacy_task_uuid`; do not build a second fuzzy matcher.

```rust
#[test]
fn duplicate_needles_resolve_to_one_identity() {
    let tasks = crate::tasks::complete::CsvFile {
        header: vec!["task_id".into(), "task_uuid".into(), "task_name".into()],
        rows: vec![std::collections::BTreeMap::from([
            ("task_id".into(), "T17".into()),
            ("task_uuid".into(), "11111111-1111-4111-8111-111111111111".into()),
            ("task_name".into(), "Send proposal".into()),
        ])],
    };
    let habits = crate::tasks::complete::CsvFile { header: vec![], rows: vec![] };
    let selected = resolve_targets(&tasks, &habits, &["T17".into(), "t17".into()]);
    assert_eq!(selected.expect("targets").len(), 1);
}
```

- [ ] **Run RED:** `cargo test --release duplicate_needles_resolve_to_one_identity`. Expect the missing resolver error.
- [ ] **Add these concrete domain enums and error envelope** in `model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Dependents { #[default] Reject, Unlink, Rewire }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkedIssue { #[default] Configured, Keep, Cancel }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Options {
    pub habit: bool,
    pub dependents: Dependents,
    pub linked_issue: LinkedIssue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

impl Problem {
    pub(crate) fn new(code: &str, message: &str, details: serde_json::Value) -> Self {
        Self { code: code.into(), message: message.into(), details }
    }
}
```

- [ ] **Implement target resolution** by calling existing `locate` for every needle before returning any target. Return a sorted/deduplicated vector of `(CsvKind, row index)`; use `CsvKind::as_str()` and numeric normalized display IDs for ordering. Convert all missing/ambiguous results into `Problem` with the original needle and candidate ID/name pairs. Identity construction belongs to Task 2, so legacy files need no schema edits.
- [ ] **Add behavioral cases:** T17/H17 bare-number ambiguity; unique name fragment; one missing target rejects a batch; duplicate aliases; invalid UUID; mixed tasks/habits; habit flag on a task; protected managed habit. Exercise current `protect_system_key` rather than copying its rule.
- [ ] **Run GREEN:** `cargo test --release tasks::removal::`. Commit the model/resolver slice with `feat: define native task removal policy` and required version files.

## Task 2: Snapshot inputs and fingerprint a plan

**Files:** Create `src/tasks/removal/{snapshot,fingerprint}.rs`; extend `model.rs`, `tests/fixture.rs`, `tests/planning.rs`.

- [ ] **Write and run RED:** construct identical canonical inputs in different insertion orders and assert equal digests, then change task bytes and assert inequality. Run `cargo test --release removal_fingerprint`.

```rust
#[test]
fn removal_fingerprint_changes_only_with_canonical_input() {
    let a = std::collections::BTreeMap::from([
        ("tasks/tasks.csv".to_owned(), b"before".to_vec()),
        ("tasks/habits.csv".to_owned(), Vec::new()),
    ]);
    let mut b = a.clone();
    assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
    b.insert("tasks/tasks.csv".into(), b"after".to_vec());
    assert_ne!(digest(&a).unwrap(), digest(&b).unwrap());
}
```

- [ ] **Implement the digest helper** and a typed canonical document. Use sorted data structures, not serialized hash maps:

```rust
use sha2::{Digest, Sha256};

pub(crate) fn digest<T: serde::Serialize>(value: &T) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(value)?;
    let result = Sha256::digest(bytes);
    Ok(result.iter().map(|byte| format!("{byte:02x}")).collect())
}
```

The canonical document contains schema version 1, workspace UUID, actor ID,
execution date, `Options`, sorted and deduplicated selected identity keys
(CSV kind, task UUID, display ID), complete task/habit before-images, a sorted map of
every scanned project metadata before-image, integration policy, and adapter
argv/environment-variable names/timeout. It excludes environment values,
operation UUIDs, and timestamps unrelated to task semantics.

- [ ] **Implement `Snapshot::read(context, today)`** under task ownership. Store raw before-images and parsed CSVs together. Inspect active and archived metadata without following symlinks. Reject malformed metadata and duplicate task IDs. Build each `Identity { csv, task_id, task_uuid, task_name }` using persisted UUID or existing deterministic legacy derivation; do not mutate counters or schema.
- [ ] **Add tests** proving preview leaves CSVs, metadata, counters, and schema unchanged; workspace/actor/date/options/metadata edits change the digest; environment secret values never appear in serialized input; a missing project produces a warning; a malformed project produces an error.
- [ ] **Run GREEN:** `cargo test --release tasks::removal::`. Commit `feat: snapshot and fingerprint task removals`.

## Task 3: Implement dependency policies as a pure graph transform

**Files:** Create `src/tasks/removal/dependencies.rs`; extend `tests/planning.rs`.

- [ ] **Write RED** with a graph `T3 -> T2 -> T1`, delete T2, and require rewire to produce `T3 -> T1`. Include batch deletion of T1/T2 and preserve unrelated blockers.

```rust
#[test]
fn rewiring_a_removed_middle_task_preserves_its_prerequisite() {
    use std::collections::{BTreeMap, BTreeSet};
    let graph = BTreeMap::from([
        ("T1".into(), BTreeSet::new()),
        ("T2".into(), BTreeSet::from(["T1".into()])),
        ("T3".into(), BTreeSet::from(["T2".into()])),
    ]);
    let removed = BTreeSet::from(["T2".into()]);
    let result = rewrite(&graph, &removed, crate::tasks::removal::model::Dependents::Rewire).unwrap();
    assert_eq!(result["T3"], BTreeSet::from(["T1".into()]));
    assert!(!result.contains_key("T2"));
}
```

- [ ] **Run RED:** `cargo test --release rewiring_a_removed_middle_task`.
- [ ] **Implement expansion** with this recursive helper, returning a sorted set at the boundary:

```rust
use std::collections::{BTreeMap, BTreeSet};
type Graph = BTreeMap<String, BTreeSet<String>>;

fn expand(
    id: &str,
    graph: &Graph,
    removed: &BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let prerequisites = graph.get(id).ok_or_else(|| format!("missing prerequisite {id}"))?;
    if !removed.contains(id) { return Ok(BTreeSet::from([id.to_owned()])); }
    if !visiting.insert(id.to_owned()) { return Err(format!("dependency cycle at {id}")); }
    let mut result = BTreeSet::new();
    for prerequisite in prerequisites {
        result.extend(expand(prerequisite, graph, removed, visiting)?);
    }
    visiting.remove(id);
    Ok(result)
}
```

- [ ] **Implement `rewrite(graph, removed, policy)`**: omit removed rows; return `dependents_exist` for any surviving incoming edge under Reject; filter only removed IDs under Unlink; replace them with `expand` under Rewire. Preserve unrelated blockers. Validate resulting changed nodes with DFS against self-reference and cycles. Convert helper errors to stable codes `missing_prerequisite` and `dependency_cycle`. Order final CSV tokens by normalized numeric task ID, not lexical T10-before-T2 order.
- [ ] **Add and run cases:** diamond deduplication, multiple consecutive deletions, cycles inside the expanded deleted set, newly exposed self-edge, missing target, and an unrelated pre-existing cycle that does not affect this deletion. Run `cargo test --release tasks::removal::dependencies` and commit `feat: repair task dependencies during removal`.

## Task 4: Clean structured references and project arrays

**Files:** Create `src/tasks/removal/{references,projects}.rs`; extend planning tests.

- [ ] **Write RED** proving an exact pipe item is removed while a URL and prose remain. Run `cargo test --release removed_task_reference`.

```rust
#[test]
fn removed_task_reference_does_not_rewrite_a_url_or_prose() {
    let removed = std::collections::BTreeSet::from(["T17".to_owned()]);
    let text = " T17 |https://example.test/T17|notes about T17|T18";
    assert_eq!(strip_items(text, &removed),
        "https://example.test/T17|notes about T17|T18");
}
```

- [ ] **Run RED:** `cargo test --release removed_task_reference`. Expect the missing `strip_items` function, then add this implementation:

```rust
fn strip_items(value: &str, removed: &std::collections::BTreeSet<String>) -> String {
    value.split('|').filter(|item| !removed.contains(item.trim()))
        .collect::<Vec<_>>().join("|")
}
```

- [ ] **Implement project cleanup** against parsed `serde_json::Value`: require an object; if `tasks` is present require an array of strings; retain entries not in the deleted set. Do not sort surviving entries or replace other metadata. Only serialize a file if the array changes. Scan reverse references even when the source task's `project` is empty.
- [ ] **Add tests** for archived metadata, stale reverse links in a second project, unknown fields, empty project after removal, malformed array, symlinked metadata, and unchanged file bytes. Do not create missing metadata or update project status.
- [ ] **Run GREEN:** `cargo test --release tasks::removal::`. Commit `feat: clean task references and project links on removal`.

## Task 5: Share gap-aware chunk resolution

**Files:** Create `src/tasks/chunk_family.rs`; modify `src/tasks/mod.rs`, `src/tasks/complete/complete_ops.rs`, `src/tasks/agenda/sync.rs`; add unit tests in the new module.

- [ ] **Write RED** for `(1/4), (3/4), (4/4)` where the successor of 1 is 3. Run `cargo test --release chunk_family`.
- [ ] **Define the resolver input and pure selection**:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chunk {
    pub id: String,
    pub base: String,
    pub index: u32,
    pub total: u32,
    pub project: String,
    pub done: bool,
}

pub(crate) fn next<'a>(anchor: &Chunk, rows: &'a [Chunk]) -> Result<Option<&'a Chunk>, String> {
    let mut candidates = rows.iter().filter(|row| {
        row.base == anchor.base && row.total == anchor.total
            && row.project == anchor.project && row.index > anchor.index && !row.done
    }).collect::<Vec<_>>();
    candidates.sort_by_key(|row| row.index);
    if candidates.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return Err("ambiguous_chunk_family".into());
    }
    Ok(candidates.first().copied())
}
```

- [ ] **Add family validation before `next`**: parse with existing `parse_chunk_name`; require a unique row per family/index; reject a cross-family dependency that would make same-title sequences ambiguous. A family key is base + original total + project. Duplicate same-project families must fail rather than be separated by guesses about assignment or creation date.
- [ ] **Replace index + 1 lookups** in completion and agenda with this shared resolver. Keep their existing completion eligibility semantics; deletion's additional visibility/unblocked checks are applied in the planner. Do not rename any surviving chunk.
- [ ] **Add cases** for first/middle/last and consecutive gaps, done successors, different projects with identical names, duplicate positions, and already-present MIT/agenda entries. Run `cargo test --release chunk_family`, `cargo test --release tasks::complete`, and `cargo test --release tasks::agenda`. Commit `fix: preserve chunk handoff across removed sessions`.

## Task 6: Compose the complete pure removal plan

**Files:** Create `src/tasks/removal/plan.rs`; extend model and planning tests.

- [ ] **Write RED** for deleting a task with a dependent, a project reverse link, and a `see_also` reference. Assert all proposed file changes without writing a file. Run `cargo test --release removal_plan`.
- [ ] **Define the service data contracts** in `model.rs`:

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Identity {
    pub csv: String,
    pub task_id: String,
    pub task_uuid: String,
    pub task_name: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct FileEdit {
    pub relative: std::path::PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ExternalAction {
    pub adapter: String,
    pub issue: String,
    pub action_id: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RemovalPlan {
    pub fingerprint: String,
    pub targets: Vec<Identity>,
    pub options: Options,
    pub edits: Vec<FileEdit>,
    pub external: Vec<ExternalAction>,
    pub successors: std::collections::BTreeMap<String, String>,
    pub blockers: Vec<Problem>,
    pub warnings: Vec<Problem>,
}
```

- [ ] **Implement `plan(snapshot, targets, options)`** in this fixed order: validate scope/protections; compute deleted identities; transform blockers; strip structured links; apply chunk MIT transfer if eligible; prune project arrays; validate external action policy; serialize only changed files; derive fingerprint. Keep a preview with blockers but no executable effect list if any step prevents safe planning.
- [ ] **Use `last_touched=today` only on changed surviving rows.** Preserve unknown CSV columns by transforming `Row` maps and serializing with the original header. Do not reset IDs, assignment, completed dates, or defer counts. MIT promotion requires `not_started` or `in_progress` and today's visibility; a nonempty `blocked_by` entry referencing an unfinished survivor prevents it. Waiting, backlog, and done successors do not receive a deletion-driven promotion.
- [ ] **Add deterministic-repeat tests** comparing serialized previews and after-images across repeated planning and duplicate target orderings. Add a table test mapping every guard to its stable code and zero canonical writes. Run `cargo test --release tasks::removal::` and commit `feat: plan complete deterministic task deletions`.

## Task 7: Extract the grouped transaction with caller-specific policies

**Files:** Move shared implementation into `src/tasks/store_transaction/{mod,artifacts,journal,tests}.rs`; adapt `src/tasks/triage_habits/transaction/{mod,artifacts,journal}.rs`; modify `src/tasks/mod.rs`.

- [ ] **Add RED** to existing transaction tests: a removal transaction includes `tasks/tasks.csv` and `archive/projects/work__sample/.METADATA.json`, then inject failure immediately after the CSV replacement and require both originals after recovery. Run `cargo test --release removal_group_recovers`.
- [ ] **Extract, do not rewrite, the existing journal algorithm.** Preserve `Preparing -> Prepared -> Committed`, fsync boundaries, compare-before-write, workspace authentication, artifact-name validation, and failure injection. Parameterize `TargetPolicy` as `ManagedTriage` or `TaskRemoval`.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetPolicy { ManagedTriage, TaskRemoval }

pub(crate) fn removal_path_allowed(path: &std::path::Path) -> bool {
    use std::path::{Component, Path};
    if path.components().any(|part| !matches!(part, Component::Normal(_))) {
        return false;
    }
    if matches!(path.to_str(), Some("tasks/tasks.csv" | "tasks/habits.csv")) {
        return true;
    }
    let project = path.starts_with(Path::new("projects"))
        || path.starts_with(Path::new("archive/projects"));
    project && matches!(path.file_name().and_then(|name| name.to_str()),
        Some(".METADATA.json" | "notes.md"))
}
```

- [ ] **Keep journal compatibility explicit.** Recover existing triage schema-v2 journals at their current path before writing a new journal. New removal journals live under the workspace UUID cache, have an explicit policy/version, and validate exact allowed target paths and workspace root/UUID. Do not reinterpret an old journal as the broader removal policy.
- [ ] **Implement failure tests** at staging, journal publication, each install/fsync, rollback, and cleanup. Recovery must yield all-before or all-after bytes. Add malformed journal, cross-workspace path, symlink, and unexpected-before-image cases.
- [ ] **Run GREEN:** `cargo test --release store_transaction` and `cargo test --release triage_habits`. Commit `refactor: share authenticated task file transactions`.

## Task 8: Coordinate canonical writers and sync publication

**Files:** Modify `src/tasks/store_lock.rs`, `src/command/project.rs`, `src/reindex/`, `src/sync/csv_sync/{mod,metadata}.rs`; add `tests/task_removal_sync.rs`.

- [ ] **Write RED** with two threads/processes: pause removal after staging and start project metadata mutation/sync publication. Assert the second writer cannot observe/publish a half-installed group. Run `cargo test --release --test task_removal_sync`.
- [ ] **Add owner-aware internal entry points** so callers already holding `TaskStoreOwner` never reacquire it. Call shared transaction recovery after acquiring ownership. In `project::set/archive`, hold ownership through canonical mutation; call derived reindex with the same owner or after releasing it according to whether it reads canonical arrays.
- [ ] **Use this lock order everywhere**:

```text
sync publication: sync lock -> task-store owner -> recover -> read/write -> release task owner
task/project mutation: task-store owner -> recover -> read/write -> release
removal external calls: operation lock only; no task-store owner during network work
```

- [ ] **Keep the CSV merge decision unchanged.** Add acceptance fixtures for local deletion vs unchanged remote row (deleted) and local deletion vs edited remote row (edit retained with conflict). Verify project arrays match whichever row survives reconciliation.
- [ ] **Run GREEN:** `cargo test --release --test task_removal_sync`, `cargo test --release project`, `cargo test --release csv_sync`, and `cargo test --release triage_habits`. Commit `fix: coordinate task deletion with project and sync writers`.

## Task 9: Execute consented local plans and retain durable receipts

**Files:** Create `src/tasks/removal/{execute,operations}.rs`, extend `model.rs`; adapt `src/tasks/mutate/remove.rs`; add execution tests.

- [ ] **Write RED**: preview T17, edit its notes, then apply the original fingerprint. Assert `stale_plan`, unchanged task membership, and no external invocation. Also test a display ID reused for a different UUID. Run `cargo test --release stale_removal_plan`.
- [ ] **Expose these service entry points** from `removal/mod.rs`. Use domain `Problem` at the surface and preserve the underlying error in internal logs without sensitive values.

```text
preview(context: &CommandContext, needles: &[String], options: Options,
        today: NaiveDate) -> Result<RemovalPlan, Problem>
apply(context: &CommandContext, plan: &RemovalPlan, expected: &str,
      today: NaiveDate) -> Result<Receipt, Problem>
resume(context: &CommandContext, operation_id: &str) -> Result<Receipt, Problem>
operation_status(context: &CommandContext, operation_id: &str)
      -> Result<Operation, Problem>
abandon(context: &CommandContext, operation_id: &str) -> Result<Operation, Problem>
```

- [ ] **Define the durable operation states** and receipt. Store operations under `workspace.paths().cache_dir()/task-removals/<uuid>.json`. Add `task_removals_dir()` to `src/workspace/paths.rs` as `self.cache_dir.join("task-removals")` and use it everywhere; do not derive HOME independently.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Prepared, ExternalPending, ExternalComplete, LocalCommitted,
    Complete, NeedsReconfirmation, Abandoned,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Receipt {
    pub operation_id: String,
    pub committed: bool,
    pub removed: Vec<super::model::Identity>,
    pub updated: Vec<String>,
    pub external: Vec<serde_json::Value>,
    pub warnings: Vec<super::model::Problem>,
}
```

`Operation` stores schema version, operation UUID, workspace UUID, actor ID,
date, original fingerprint, serialized canonical plan inputs, before/after
edits, phase, per-action external status, and optional receipt. JSON records
are mode 0600 and atomically replaced under an operation-specific lock.
Validate UUID syntax before constructing a path.

- [ ] **Implement the local-only execution path:** acquire operation ownership; acquire task ownership; recover pending local transaction; recompute and compare plan/identities; persist `Prepared`; execute grouped changes; persist an immutable local-commit receipt; release task ownership; refresh agenda; persist `Complete` with warnings. Use the existing agenda sync for the initial single-task red/green slice; Task 13 replaces this adapter with the explicit batch removal refresh before final acceptance. The committed transaction journal remains until `task-removals/<uuid>.commit.json` is durable. Recovery writes that separate immutable receipt under task ownership without acquiring an operation lock; status/resume treat it as authoritative over the mutable operation progress file. This avoids reversing the operation-lock-before-task-lock order during recovery.
- [ ] **Add failure tests** for crash before commit, after commit but before receipt, repeated resume, wrong-workspace operation ID, unavailable operation, and abandon-after-commit refusal. Add a concurrent recovery/resume test proving no operation/task lock inversion. A completed resume returns the original receipt without touching data.
- [ ] **Run GREEN:** `cargo test --release tasks::removal::` and `cargo test --release store_transaction`. Commit `feat: execute and recover native task removals`.

## Task 10: Add deterministic integration configuration and protocol

**Files:** Create `src/tasks/removal/integration/{mod,config,protocol,process}.rs`; modify `src/config.rs`, `src/settings/{schema,vars}.rs`, `src/env/schema.rs`; add protocol/process tests and settings docs.

- [ ] **Write RED** for absent policy resolving to keep, missing adapter under explicit cancel, and a shared issue on a surviving task. Run `cargo test --release removal_integration_policy`.
- [ ] **Add exact config shapes** with serde `deny_unknown_fields` and validation:

```json
{
  "task_removal_integration": {
    "adapter": "tracker",
    "link_field": "linear_issue",
    "on_remove": "cancel"
  }
}
```

```json
{
  "task_removal_adapters": {
    "tracker": {
      "argv": ["/opt/tools/tracker-removal"],
      "env_names": ["TRACKER_API_TOKEN"],
      "timeout_seconds": 30
    }
  }
}
```

Portable policy is absent by default; env adapters default to an empty map.
Require a nonempty argv, absolute executable path, unique valid environment
names, and timeout 1-120 seconds. Never accept literal credential values in
the portable object. `on_remove` accepts keep/cancel; v1 `link_field` accepts
only `linear_issue`. The command uses selected-workspace env provenance.

- [ ] **Implement the protocol DTOs** and strict response matching:

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub schema_version: u32,
    pub operation_id: String,
    pub action_id: String,
    pub workspace_id: String,
    pub actor_id: String,
    pub action: String,
    pub issue: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Response {
    pub schema_version: u32,
    pub operation_id: String,
    pub action_id: String,
    pub status: String,
    pub message: Option<String>,
}
```

Allowed action/status pairs are preflight/ready, cancel/canceled,
cancel/already_canceled, status/canceled, status/already_canceled,
status/pending, and any action/error. Reject all other pairs, extra output,
wrong identity/version, invalid JSON, and nonzero process exit as typed
adapter errors.

- [ ] **Implement bounded process transport** with `Command::new(argv[0]).args(&argv[1..])`, explicit environment allowlisting, piped stdin/stdout/stderr, no shell, a 64 KiB request/response limit, and timeout kill-and-wait. A reader thread must drain bounded output while the coordinator polls `try_wait`; never block indefinitely on a full child pipe. Report stderr only after redacting forwarded secret values.
- [ ] **Test fake executables** for success, malformed JSON, wrong action ID, timeout, excessive output, secret redaction, and a target name containing shell metacharacters. Verify preview launches zero processes. Run `cargo test --release removal_integration` and commit `feat: add deterministic removal integration adapters`.

## Task 11: Implement external progress, resume, and abandonment

**Files:** Extend `src/tasks/removal/{execute,operations}.rs` and `tests/integration.rs`.

- [ ] **Write RED** for two issues: first cancellation succeeds, second fails. Assert both local tasks remain, action 1 is recorded successful, and resume never repeats a confirmed cancellation. Run `cargo test --release removal_external_resume`.
- [ ] **Implement this state transition table** under the operation lock:

| Current state | Event | Next state and effect |
|---|---|---|
| Prepared | All adapter preflights ready | ExternalPending; persist before first cancel |
| ExternalPending | One cancel acknowledged | Persist that action's success, continue stable order |
| ExternalPending | Unknown/failed outcome | Retain local tasks and per-action state; return operation ID |
| ExternalPending | Resume | Query status for unknown actions; retry only confirmed unfinished actions |
| ExternalPending | All actions successful | ExternalComplete |
| ExternalComplete | Local before-images still match | Commit local group, then LocalCommitted |
| ExternalComplete | Local/policy/identity changed | NeedsReconfirmation; no local mutation |
| LocalCommitted | Derived refresh completes or warns | Complete with receipt |
| Complete | Resume | Return existing receipt |
| Uncommitted, no active/unknown action | Explicit abandon | Abandoned; preserve local rows and external history |

- [ ] **Keep network work outside task ownership.** Reacquire ownership for the second complete snapshot check. Do not silently recompute different effects under old consent. Use `shared_external_link` to reject cancellation if a nonselected row whose status is not `done` shares an issue. Dedupe selected issue references before building actions.
- [ ] **Add tests** for crash after remote success before local acknowledgement, provider already-canceled response, completed-state conflict, local edits during network call, changed adapter configuration, actor mismatch, active-operation abandonment, and wrong-workspace resume. An unknown result remains unknown until status/retry establishes it.
- [ ] **Run GREEN:** `cargo test --release removal_external` and `cargo test --release tasks::removal::operations`. Commit `feat: resume partial external task removals safely`.

## Task 12: Add the CLI grammar, preview, confirmation, and JSON shell

**Files:** Modify `src/tasks/cli.rs`, `src/command/tasks/mutate/mod.rs`; create `src/command/tasks/mutate/remove/{mod,interactive,report}.rs`; create `tests/task_removal_cli.rs`.

- [ ] **Write RED** in `src/tasks/cli.rs`:

```rust
#[test]
fn removal_accepts_explicit_batch_policy_and_consent() {
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "tasks", "remove", "T1", "T2", "--dependents", "rewire",
        "--linked-issue", "keep", "--yes", "--json",
    ]).expect("parse deterministic removal");
    let Some(Command::Remove(args)) = cli.command else { panic!("remove command") };
    assert_eq!(args.ids, ["T1", "T2"]);
    assert!(args.yes && args.json);
}
```

- [ ] **Run RED:** `cargo test --release removal_accepts_explicit_batch_policy`.
- [ ] **Replace `RemoveArgs`** with explicit multi-target/operation grammar:

```rust
#[derive(clap::Args, Debug)]
pub struct RemoveArgs {
    #[arg(num_args = 1.., required_unless_present_any = ["resume", "operation_status", "abandon"])]
    pub ids: Vec<String>,
    #[arg(long)] pub habit: bool,
    #[arg(long, value_enum)] pub dependents: Option<crate::tasks::removal::model::Dependents>,
    #[arg(long, value_enum)] pub linked_issue: Option<crate::tasks::removal::model::LinkedIssue>,
    #[arg(long)] pub dry_run: bool,
    #[arg(long)] pub json: bool,
    #[arg(long)] pub yes: bool,
    #[arg(long, conflicts_with = "dry_run")] pub expect_plan: Option<String>,
    #[arg(long, conflicts_with_all = ["ids", "operation_status", "abandon"])]
    pub resume: Option<String>,
    #[arg(long, conflicts_with_all = ["ids", "resume", "abandon"])]
    pub operation_status: Option<String>,
    #[arg(long, conflicts_with_all = ["ids", "resume", "operation_status"])]
    pub abandon: Option<String>,
}
```

Make the value enums public only if the public clap DTO requires it; keep the
service implementation crate-private. Validate remaining combinations in one
pure `validate_mode` function: operations reject habit/dependency/issue,
dry-run and fingerprint flags; resume/status reject yes; abandon requires yes.
Absence of explicit policy flags uses defaults. Reject inherited list filters
that imply target selection.

- [ ] **Implement the shell decision**: dry-run emits preview; JSON/nonterminal apply without yes returns confirmation_required; terminal apply shows preview, fixed policy choices if blocked, then confirmation; confirmed apply passes the displayed fingerprint. `--yes` cannot choose a dependency policy. `--expect-plan` must equal the freshly built plan before invocation.
- [ ] **Implement JSON DTO conversion** without serializing raw file before-images. Emit the spec's versioned envelope. Route typed validation errors to exit 2 and operational failures to 1 without printing a second top-level error. Preserve clap's syntax-error output.
- [ ] **Add black-box CLI tests** for aliases, batches, no-tty/no-consent, dry-run immutability, stale fingerprint, protected habits, mixed scope, JSON stdout, human stderr, and operation modes. Run `cargo test --release --test task_removal_cli` and commit `feat: expose deterministic removal commands and previews`.

## Task 13: Refresh agendas with explicit removal effects

**Files:** Modify `src/tasks/agenda/{mod,sync}.rs`; add `src/tasks/agenda/tests/remove.rs`; extend `execute.rs`.

- [ ] **Write RED** deleting `(2/4)` from the CSV while its before-image remains in the plan. Assert that a supplied eligible `(3/4)` successor takes the intended slot exactly once. Run `cargo test --release tasks::agenda::tests::remove`.
- [ ] **Add a batch removal input** instead of relying on a deleted row in the post-mutation snapshot:

```rust
#[derive(Clone, Debug)]
pub(crate) struct RemovalAgendaChange {
    pub removed_id: String,
    pub successor_id: Option<String>,
}
```

The caller supplies one vector of these records from `RemovalPlan.targets`
and `RemovalPlan.successors`. The synchronizer drops matching MIT/Suggested/
Cut entries, performs the explicit replacement where supplied, renumbers
once, and derives current habits/completed sections once. Reuse existing
`lines` helpers and original-section preservation; do not freehand-render a
new agenda.

- [ ] **Return an explicit derived result** from removal refresh: no agenda, unchanged, updated, or warning with the failed stage. Translate a PDF generation error into a receipt warning after local commit. Preserve ordinary completion's current output contract.
- [ ] **Test** batch removal, absent agenda, unrelated appendix preservation, missing/deferred/backlogged/blocked successor, duplicate successor callout, completed-history removal, and PDF failure. Run `cargo test --release tasks::agenda` and `cargo test --release --test agenda_sync_cli`. Commit `feat: synchronize agendas after task deletion batches`.

## Task 14: Wire the TUI and maintenance callers to the shared operation

**Files:** Create `src/tui/task_removal/{mod,state,worker,render}.rs`; modify the TUI files in the map, `src/command/backlog.rs`, and `src/tasks/backlog/{purge,dedupe}.rs`; add focused TUI tests.

- [ ] **Write RED** in a TUI action test: invoke Remove, confirm, and assert the injected removal service is called while the brain-prompt recorder remains empty. Repeat with no available agent controller. Run `cargo test --release native_task_removal`.
- [ ] **Model the removal overlay** with these states:

```rust
pub(crate) enum RemovalScreen {
    Preview(crate::tasks::removal::model::RemovalPlan),
    Running { operation_id: Option<String> },
    Failed(crate::tasks::removal::model::Problem),
    Finished(crate::tasks::removal::operations::Receipt),
}
```

Expose an injected native service in `src/tui/state/services.rs`, using the
same pattern as existing injected services. The key/palette action opens a
preview; dependency choices rebuild it; confirm launches the native worker
with the saved fingerprint. Route worker results by a monotonic request ID so
a late result cannot overwrite another modal. No UI worker owns a frontend.

- [ ] **Replace `run_remove`'s prompt completely.** Keep the current keybinding and palette target picker. Update overlay routing/drawing and tick polling; preserve main-view/focus state. On success reload tasks and choose nearest surviving row. On stale error rebuild preview without confirming it automatically. Expose status/resume/abandon for the failed operation in the removal modal, with abandon's consequence shown explicitly.
- [ ] **Write RED for maintenance before changing it:** backlog purge removes an expired task and rewires an active dependent; dedupe redirects references to the uniquely selected survivor and keeps the external issue. Run `cargo test --release backlog_removal_cleanup`.
- [ ] **Replace independent maintenance writes.** Keep cutoff/dedupe candidate calculation pure. Extend `Superseded` to carry `survivor_id`; skip multiple eligible twins. Build purge plans with rewire/keep and breadcrumb edits; build dedupe plans with duplicate-to-survivor reference substitution/keep. Preserve silent success and existing report flags. Never pass habit opt-in from triage/backlog paths.
- [ ] **Test** TUI cancellation, policies, stale preview, background result routing, batch cleanup, purge breadcrumbs, and unambiguous dedupe; ensure command/key parity tests still pass. Run `cargo test --release tui::`, `cargo test --release backlog`, and `cargo test --release --test task_removal_cli`. Commit `feat: remove tasks natively from the TUI and maintenance flows`.

## Task 15: Rewrite bundled skill deletion instructions and validate full commands

**Files:** Modify `skills/todo/SKILL.md`, `skills/todo/references/{commands,task-project-link}.md`, `skills/triage/SKILL.md`, and relevant task references in `skills/second-brain/SKILL.md`; create `tests/skill_command_examples.rs`; extend `tests/bundled_skill_commands.rs`.

- [ ] **Write RED** asserting the bundled todo instructions contain the deterministic preview/apply contract and no `status=dropped`, manual CSV removal, or subjective link-preservation branch. Add complete argv parsing tests before adding the prose. Run `cargo test --release --test skill_command_examples`.

```rust
#[test]
fn documented_removal_options_parse() {
    use clap::Parser;
    let examples = [
        vec!["tasks", "remove", "T17", "--dry-run", "--json"],
        vec!["tasks", "remove", "T17", "--yes", "--json"],
        vec!["tasks", "remove", "T17", "T18", "--dependents", "rewire", "--yes"],
        vec!["tasks", "remove", "H42", "--habit", "--yes"],
        vec!["tasks", "remove", "T17", "--linked-issue", "keep", "--yes"],
    ];
    for example in examples {
        assert!(brain::tasks::cli::Cli::try_parse_from(example).is_ok());
    }
}
```

- [ ] **Replace the core remove section with this text**, adapting only neighboring link anchors:

```markdown
### Remove tasks

Deletion is implemented by `brain tasks remove`. Use a preview to identify
the exact rows and effects:

    brain tasks remove T17 --dry-run --json

After explicit consent, apply the chosen policy with `--yes`. If consent was
based on a preview, pass its `fingerprint` as `--expect-plan`. A stale plan
requires a fresh preview and consent. Do not edit CSV rows or project arrays
yourself.

    brain tasks remove T17 --yes --json
    brain tasks remove T17 T18 --dependents rewire --yes --json

`reject` is the default dependency policy. `unlink` removes references to the
deleted tasks; `rewire` preserves their surviving prerequisites. These options
do not delete dependent tasks. An unresolved policy is a user choice, not an
invitation to invent one. The TUI collects the same choices natively.

Habit removal requires explicit retirement intent and `--habit`. Managed
triage habits remain protected. Preserve completed habit history unless it
was explicitly selected.

Configured external cancellation is performed by a deterministic adapter.
Use `--linked-issue keep` for explicitly local-only removal, such as a
reassigned issue or a duplicate. Never substitute an agent-tool cancellation
when the configured adapter fails. Report the operation ID and use the native
status/resume commands; committed deletion with an agenda warning is still a
committed deletion.
```

- [ ] **Update triage and conversion references**: bulk drop passes all consented IDs in one command; link cleanup belongs to the CLI; purge/dedupe keep their specialized commands; conversion reports incomplete source removal rather than deleting by hand. Keep archive-not-delete rules for notes/projects intact.
- [ ] **Expand command validation** to parse concrete example argv through the top-level global parser and delegated task parser. Include global `-w sample`, operation modes, hashes, and illegal option combinations. Do not rely only on searching two words after `brain`. Treat examples with shell variables as documented templates, while checking matching concrete vectors in the test table.
- [ ] **Run GREEN:** `cargo test --release --test bundled_skill_commands`, `cargo test --release --test skill_command_examples`, and `cargo test --release skills::embed`. Commit `docs: teach skills the native task deletion contract`.

## Task 16: Make managed skill installation recoverable and preserve ownership

**Files:** Create `src/skills/install_transaction.rs`; modify `src/skills/{install,prune,layout}.rs`; add install tests.

- [ ] **Write RED**: inject failure during a todo tree replacement and assert the old complete tree is still recoverable, including its references and frontend links. Run `cargo test --release managed_skill_install_recovers`.
- [ ] **Replace clear-then-write installation** with staging under the same filesystem, a per-workspace skills-install lock, and a versioned recovery manifest. Build all content first and validate required `SKILL.md`/reference files before replacing live managed paths. Record original paths/backups, perform renames, publish the manifest, then mark success.

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedSkillManifest {
    pub schema_version: u32,
    pub workspace_id: String,
    pub binary_version: String,
    pub compatibility_revision: u32,
    pub files: std::collections::BTreeMap<String, String>,
}
```

`files` maps workspace-relative managed paths to SHA-256 digests. It excludes
extension/plugin source files and unrelated user-authored skills. Before the
first manifest, back up version-marked generated trees to the workspace cache.
Preserve non-owned extra files; a same-name unmarked user directory is a
conflict. On later installs compare current managed bytes to the prior digest
before replacement and report edits rather than erasing them silently.

- [ ] **Preserve customization correctly**: read `.config/extensions` and `.config/plugins`; keep their bytes untouched; render extensions into staged copies. Detect same-name core plugin collisions before writing. Use existing frontend fan-out plus relative link targets; repair only managed links, not global registries or unrelated workspace skills.
- [ ] **Recover under the skills lock** before any subsequent installation. A staged/incomplete install restores or finishes its recorded tree, never marks partial content current. Do not put arbitrary user paths from a corrupt journal into rename/delete calls.
- [ ] **Test** failure at staging, backup, each rename, link refresh, manifest publication, and final stamp; user plugin/extension preservation; reference-file changes; edited managed file; unmarked collision; and two concurrent installers. Run `cargo test --release skills::` and commit `fix: install managed skills with recovery and ownership checks`.

## Task 17: Reconcile skill compatibility across registered workspaces

**Files:** Create `src/skills/reconcile.rs`; modify `src/skills/{mod,command}.rs`, `src/cli/configuration.rs`, `src/command/configuration.rs`, and the workspace invocation/bootstrap classifiers; add `tests/task_deletion_skill_migration.rs`.

- [ ] **Write RED** using two registered roots with different extensions and `skills_auto_sync=false`: both managed todo copies must update, both extension sources must remain byte-identical, and neither global registry may change. Run `cargo test --release --test task_deletion_skill_migration`.
- [ ] **Implement explicit outcomes**:

```rust
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum WorkspaceRefresh {
    Current { workspace_id: String },
    Updated { workspace_id: String },
    PendingUnavailable { workspace_id: String, root: std::path::PathBuf },
    Failed { workspace_id: String, message: String },
}
```

Load the registry without ordinary selected-workspace bootstrap. Iterate
canonical workspace names in stable order, validate root/manifest identity,
skip unavailable roots without creating directories, and attempt every
available root even after another fails. Stamp `(binary version,
compatibility revision, managed manifest)` only after a successful install.
Missing/modified managed artifacts override an otherwise-current version
stamp. Keep pending status per workspace, not one global done marker.

- [ ] **Add CLI repair options**:

```rust
Sync {
    #[arg(long, conflicts_with = "all_workspaces")]
    root: Option<std::path::PathBuf>,
    #[arg(long)]
    all_workspaces: bool,
    #[arg(long)]
    json: bool,
}
```

Classify all-workspace sync as registry-only so a broken default workspace
cannot prevent repairing another root. Reject an explicit `-w` together with
`--all-workspaces`; retain selected-workspace sync and sandbox behavior.
JSON aggregates all outcomes in one document. Return nonzero for available
root failures, success with pending-unavailable summaries for missing roots.

- [ ] **Unify startup/version refresh** around this coordinator. Mandatory bundled compatibility revision 1 ignores the routine auto-sync gate without changing its configured value. Optional config/persona refreshes still respect it. TUI startup uses the same revision check instead of unconditional duplicate writes; capability renders use the current embedded source at next launch.
- [ ] **Test** missing registry, legacy registry normalization, unavailable/remounted root, malformed manifest, one failed/one successful root, same-version missing file repair, duplicate aliases, and retry after a failed stamp. Run the migration integration test plus `cargo test --release skills::`. Commit `feat: reconcile managed skill compatibility across workspaces`.

## Task 18: Add automatic migration and target-binary downgrade refresh

**Files:** Create `src/startup_migration/task_deletion_skills.rs`; modify `src/startup_migration/mod.rs`, `src/cli/{internal,mod}.rs`, `src/main.rs`, `install.sh`, and `tests/install_script.rs`; extend migration tests.

- [ ] **Write RED in the installer test fixture**: on downgrade, expect old installed binary down-migration before replacement, then the retained migration helper invoking the target binary's selected-workspace skill sync after replacement. The target is deliberately a stub that rejects new flags. Run `cargo test --release --test install_script`.
- [ ] **Add up/down callbacks** at the first release version containing this feature. Up calls the coordinator with compatibility revision 1. Down checks for unrecoverable active removal operations, settles local recovery only, and invalidates compatibility markers. It never calls an adapter or labels new skill content with the target's old version.
- [ ] **Add a hidden installer-only coordinator command**:

```text
brain __refresh-workspace-skills --target-binary /absolute/path/brain
  --target-version 0.101.0 --phase preflight
brain __refresh-workspace-skills --target-binary /absolute/path/brain
  --target-version 0.101.0 --phase apply
```

The argument type is `RefreshWorkspaceSkillsArgs { target_binary: PathBuf,
target_version: String, phase: RefreshPhase }`, with `RefreshPhase` a clap
ValueEnum of Preflight/Apply. This command runs before ordinary bootstrap.
Preflight checks target version, supported `-w NAME skills sync` grammar,
workspace identity/schema compatibility, absence of unresolved operations,
and availability summaries. Apply revalidates identity and calls the target
binary per available canonical name with an argv vector:

```rust
let status = std::process::Command::new(&args.target_binary)
    .args(["-w", workspace_name.as_str(), "skills", "sync"])
    .status()?;
```

`workspace_name` comes from the registry, never task text. Do not use
`--root`; the current sandbox route reads `root/extensions` and `root/plugins`
instead of real `.config` customization. Do not pass `--all-workspaces` or
new removal flags to an older binary. Preflight must fail before replacement
if the target cannot safely support selected-workspace sync.

- [ ] **Modify installer ordering** using a retained executable copy of the migration-capable binary. For upgrades/reinstalls, use the built new binary as coordinator. For downgrades, copy the installed newer binary before replacement and use that copy for preflight/down/post-refresh. Preserve existing migration order, run post-refresh only after the target is installed, and clean the temporary helper on every exit. A post-refresh failure returns nonzero with per-workspace pending details; it does not falsely print installation fully complete.
- [ ] **Test the actual protocol** with stubs recording executable identity and argv: upgrade, same-version reinstall, fresh install, supported downgrade, unsupported old CLI, a target with a different embedded skill body, failed root, missing root, and paths containing spaces. Unit-test up/down idempotence separately. Run `cargo test --release --test install_script` and `cargo test --release --test task_deletion_skill_migration`. Commit `feat: migrate workspace skills on install and downgrade`.

## Task 19: Update behavior docs and prove installed instructions match

**Files:** Modify `docs/{features,architecture,data-model,integrations,config,keybindings,testing,decisions}.md`, CLI long help, and `tests/skill_command_examples.rs`.

- [ ] **Write RED** for rendered installed skills, not just source: use a temporary workspace with an extension, run the real install pipeline, and assert the output includes new commands and preserves the extension. Parse its concrete examples using the same argv tests. Run `cargo test --release installed_deletion_skill_contract`.
- [ ] **Document exact user-visible behavior** with these required examples:

```sh
brain tasks remove T17 --dry-run --json
brain tasks remove T17 --dependents rewire --yes
brain tasks remove T17 T18 --linked-issue keep --yes --json
brain tasks remove H42 --habit --yes
brain skills sync --all-workspaces --json
```

Show a real-shaped fingerprint/operation ID in explanatory JSON, explain exit
statuses and partial external effects, and document status/resume/abandon.
Explain that confirmation does not imply agent use and that there is no
recursive cascade deletion. Include the source-to-render ownership boundary
and mandatory compatibility refresh despite routine auto-sync opt-out.

- [ ] **Update architecture and data model** for RemovalPlan/Receipt, operation states, local transaction acknowledgement, writer lock ordering, chunk family ambiguity, and the optional adapter protocol. Document unchanged sync delete/edit semantics and target-binary downgrade refresh. Remove stale CLI help claiming native completion delegates to an agent because it would misdescribe the shared native-operation precedent.
- [ ] **Update key descriptions without changing shortcuts.** Keep the shortcut table, footer, pure classifier, palette annotation, and docs aligned. The Remove entry now describes native preview/confirmation, not an agent handoff.
- [ ] **Run GREEN:** `cargo test --release installed_deletion_skill_contract`, `cargo test --release --test bundled_skill_commands`, `cargo test --release --test skill_command_examples`, and the shortcut parity tests. Commit `docs: document native deletion and automatic skill compatibility`.

## Task 20: Run acceptance, review the final diff, and prepare the release

**Files:** `tests/task_removal_cli.rs`, `tests/task_removal_sync.rs`, `tests/task_deletion_skill_migration.rs`, existing transaction/TUI/install suites; version files and relevant docs for any final fixes.

- [x] **Run the canonical acceptance scenario** in an isolated workspace: create three chained tasks plus an archived project, preview deleting the middle task, confirm rewire, verify task CSV/project metadata/agenda, then exercise the same plan through the TUI service fixture. Compare canonical after-images and assert no agent prompt was sent.
- [x] **Run the integration scenario** with a fake adapter: partial remote failure leaves local rows; resume recognizes earlier cancellation; canonical commit occurs once; PDF failure reports a committed warning; repeated resume returns the stored receipt.
- [x] **Run upgrade/downgrade acceptance** with two registered roots, different extensions, routine auto-sync disabled, one temporarily missing root, and older target CLI stubs. Verify the target binary supplies the actual rendered content on downgrade and each workspace retains its own customization.
- [x] **Run required checks**:

```sh
cargo fmt --check
cargo test --release
cargo clippy --release --all-targets -- -D warnings
git diff --check
```

Run the configured personal-data guard as required by the execution
environment. The inspected baseline has bundled-skill genericity/build-litter
tests under `skills::embed`, but no separately named personal-data script;
do not invent a successful guard command. Locate the actual configured hook
before publication, and if absent record that fact for the release owner.
Review the diff for machine paths, private extensions, tokens, and live data.

- [x] **Review requirement coverage** using the matrix below, inspect the final diff, and confirm no removal path still emits `Remove ... via the /todo remove path` or offers `status=dropped`. Distinguish fake-adapter acceptance from provider deployment. Follow repository release/installation authorization only after all required checks pass.

## Task 21: Deploy an existing tracker integration through the adapter contract

**Ownership:** User-owned integration repository/plugin, not public Brain core. This is a separate rollout task with the protocol from Task 10 as its dependency. Do not include user names, issue identifiers, credentials, or private paths in public commits.

**Files:** For the existing Linear integration, resolve the selected workspace
root as `BRAIN_ROOT`. Create
`$BRAIN_ROOT/.config/plugins/linear-sync/scripts/removal-adapter.py`,
`$BRAIN_ROOT/.config/plugins/linear-sync/scripts/linear-client.py`, and
`$BRAIN_ROOT/.config/plugins/linear-sync/tests/test-removal-adapter.py`.
Update that plugin's `SKILL.md`, `references/linear-link.md`, and
`$BRAIN_ROOT/.config/extensions/{todo,triage}.md`. Store the canceled-state
mapping in the plugin's `config/removal.json`; never edit rendered frontend
copies. These paths are runtime-owned integration files, not public core
files. They must be edited in the appropriate workspace/worktree under its
own rules during execution.

- [ ] **Write failing contract tests** against a fake provider client: preflight validates access; cancel maps to the provider's canceled state; repeated cancel returns already_canceled; completed state returns a conflict; reassigned/local-only removal never calls cancel. Use fixed synthetic issue IDs and operation/action UUIDs.
- [ ] **Implement the adapter's provider boundary** using Python's standard-library HTTPS client and Linear GraphQL. Linear documents the endpoint, API-key header, `issueUpdate`, and checking GraphQL errors even with HTTP 200 in its [GraphQL guide](https://linear.app/developers/graphql). Workflow states are [team-specific](https://linear.app/docs/configuring-workflows); require an explicit canceled-state UUID per team instead of guessing among multiple canceled statuses. These external contracts were checked on 2026-10-14 and must be rechecked when implementation starts.

Use this transport in `linear-client.py`:

```python
import json
import urllib.request

def graphql(query, variables, api_key):
    request = urllib.request.Request(
        "https://api.linear.app/graphql",
        data=json.dumps({"query": query, "variables": variables}).encode(),
        headers={"Content-Type": "application/json", "Authorization": api_key},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        payload = response.read(65537)
    if len(payload) > 65536:
        raise RuntimeError("provider_response_too_large")
    decoded = json.loads(payload)
    if decoded.get("errors") or not isinstance(decoded.get("data"), dict):
        raise RuntimeError("provider_graphql_error")
    return decoded["data"]
```

Keep the API key in the forwarded `LINEAR_API_KEY` environment variable.
Load the client via `importlib.util.spec_from_file_location` because the
workspace's required kebab-case filename is not an ordinary Python import
name. `removal-adapter.py` is the executable boundary; it validates protocol
input, loads `config/removal.json` relative to its own resolved path, catches
transport/protocol exceptions into one error response, and never prints raw
provider payloads or credentials.

Implement `provider_action` as a pure dispatch function and test it first:

```python
def provider_action(action, state_type):
    if action not in {"preflight", "status", "cancel"}:
        raise ValueError("invalid_action")
    if state_type == "canceled":
        return "ready" if action == "preflight" else "already_canceled"
    if state_type not in {"triage", "backlog", "unstarted", "started"}:
        return "error"
    if action == "preflight":
        return "ready"
    if action == "status":
        return "pending"
    return "perform_cancel"
```

Fetch the issue by identifier/UUID, obtain its immutable ID, state type, and
team ID, then validate that the configured state belongs to that team and
has canceled type. Only `perform_cancel` invokes `issueUpdate(id, input:
{stateId})`. Require `success=true` and a confirming fetch before replying.
Use the following state rules:

```text
preflight: fetch issue -> verify access and cancellable-or-already-canceled state -> ready/error
status: fetch issue -> canceled/already_canceled if canceled; pending if cancellable; error otherwise
cancel: fetch issue -> already_canceled if canceled;
        error if completed or access denied;
        otherwise apply canceled state, fetch again, return canceled only after verification
```

The adapter must implement Request
and Response exactly, including IDs, limits, timeout, and no stdout chatter.
Do not launch an agent or ask it to resolve provider state names. If the
provider's canceled-state mapping is ambiguous, require explicit adapter
configuration before enabling it.

- [ ] **Configure the logical policy and machine executable** through `brain config` and `brain env` using the shapes in Task 10. Verify configuration can be inspected without printing secrets. Core defaults remain unchanged for users without this integration.
- [ ] **Replace agent cancellation instructions** with native `brain tasks remove` calls. Use `configured` for an explicitly abandoned linked task, and `keep` for reassignment-away, already-canceled mirrors, and deduplication. Retain user choice about abandoning work; eliminate agent judgment from the deletion effects.
- [ ] **Run contract tests and fake-provider end-to-end deletion**, then `brain skills sync` in the selected test workspace. A real provider smoke test requires a specifically authorized disposable issue; absence of that authorization does not justify touching a real task. Record fake-provider versus live-provider validation accurately.

Run the plugin tests with:

```sh
python3 -m unittest discover -s "$BRAIN_ROOT/.config/plugins/linear-sync/tests" -p 'test-*.py'
```

The tests inject an HTTPS transport replacement and synthetic responses;
they must not require a live API key. Include HTTP 401/429/5xx, GraphQL errors
with HTTP 200, missing issue, wrong-team state, completed issue, repeat cancel,
timeout after mutation, protocol ID mismatch, and empty stdout on internal
debug logging. The configured adapter argv uses an absolute Python executable
plus the canonical `removal-adapter.py` path, with only the required machine
environment names forwarded.

## Requirement-to-task acceptance matrix

| Requirement | Implementing tasks | Primary evidence |
|---|---|---|
| R1 identity | 1, 2, 6, 9 | Ambiguity, duplicate needles, UUID reassignment, stale plan tests |
| R2 relationships | 3, 4, 6, 14 | Rewire/unlink/reject, project/cross-reference, dedupe tests |
| R3 chunks | 5, 6, 13 | Gap-aware completion, MIT eligibility, agenda before-image tests |
| R4 habits | 1, 6, 12, 14 | Opt-in, mixed scope, protected triage, maintenance exclusion |
| R5 transactions | 2, 6-9 | Fingerprint and every-boundary crash recovery |
| R6 truthful outcomes | 9, 11-13 | Partial remote effects, durable receipt, derived warnings |
| R7 CLI | 12, 15 | Full argv, JSON/exit status, consent and operation modes |
| R8 TUI | 14 | Shared service, no-agent recorder, stale/background state |
| R9 integration | 10, 11, 21 | Protocol/fake-provider tests and separately recorded deployment |
| R10 maintenance | 14, 15 | Silent purge, breadcrumbs, unique survivor, conversion wording |
| R11 skills | 15, 19 | Source and rendered command examples parse |
| R12 install | 16-18 | Real install-protocol and multi-workspace refresh fixtures |
| R13 recovery/downgrade | 16-18 | Ownership, retry, missing roots, target-binary render |
| R14 isolation/sync | 2, 7, 8, 17 | Workspace authentication, writer coordination, merge regressions |

## Execution handoff

The core plan is ready for either Superpowers subagent-driven execution or
inline executing-plans. Keep Task 21's provider rollout separate from public
core commits. Do not claim tracker-enabled deployment complete merely because
the generic fake-adapter suite passes. No production deletion, installation,
or implementation is part of preparing this plan.
