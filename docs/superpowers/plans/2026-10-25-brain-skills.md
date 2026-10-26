# Brain Skills Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Install concise shared workspace instructions and focused Brain skills,
including `/agenda`, with automatic reversible compatibility and central channel output.

**Architecture:** Extend the recoverable managed-skill package to own a delimited
AGENTS region and narrow compatibility patches. Share one extension projection
and channel-policy implementation across installation, actor capability rendering,
AgentController launches, and installer target replicas. Keep legacy names as
conditional compatibility routes while making focused skills the canonical owners.

**Tech Stack:** Existing Rust 2024, serde/serde_json, SHA-256, filesystem snapshots,
SQLite receipts, embedded Markdown, native agenda renderer, and existing frontend adapters.

**Status:** Approved on 2026-10-25. Execution progress is tracked in the plan's SDD ledger.
**Spec:** [Brain workspace instructions and focused skills](../specs/2026-10-25-brain-skills-design.md).
**Review:** [Pre-refactor findings](../../investigations/2026-10-25-brain-skills-review.md).
**Baseline:** `b04a64c7`, version `0.105.3`.

## Global Constraints

- Feature migration boundary is `0.106.0`; managed compatibility revision is `2`.
- Every production behavior change starts with a failing test observed before implementation.
- No new dependency and no unsafe Rust are required.
- Every registered frontend participates: Claude, Codex, OpenCode, and pi.
- Channels are exactly `interactive`, `sms`, and `email`, selected from `ActorContext`.
- SMS final replies contain at most 480 Unicode characters after Markdown removal.
- Core instructions contain no personal schedule, personal habit ID, private integration, or extension-specific output path.
- An absent extension is a correct no-op; extensions and plugins remain user-owned sources.
- Help and version are side-effect free; ordinary commands reconcile before dispatch.
- Missing registered roots remain pending and are never invented by this migration.
- Unknown user bytes, modified managed content, and filename collisions are never silently overwritten.
- Publication, recovery, and downgrade must leave a coherent instruction-and-skill generation before ordinary dispatch.
- One failed workspace does not prevent attempts on the other registered workspaces.
- No real SMS, email, or external integration is invoked by verification.

Do not raise an existing workspace's minimum version for this split. The portable
contract uses legacy fallbacks; a local version-floor change is not a distributed
sync fence. Do not push, merge, or publish as a side effect of executing this plan
unless the applicable session authorization permits that Git action.

## Execution and commit policy

Run in the existing `refactor/brain-skills` worktree. Before production changes,
finish the repository's required architecture orientation and load Rust/TDD skills.
Use `wt` for any worktree operation. Independent implementation tasks are executed
sequentially, with review after each task if using subagent-driven-development.

Each task below has one independently reviewable deliverable and a red/green
cycle. Run the named tests, observe the intended failure, then implement only that
behavior. Documentation and fixtures accompany their owning task. Refactor with
green tests; do not repeatedly rerun unrelated suites after unchanged results.
For each filtered test command, verify that the intended tests actually ran;
a successful command reporting zero matching tests is not validation.

Keep local task progress in the execution skill's ledger. Stage only named task
files. The repo requires a version bump for every commit, including documentation:
the first implementation commit uses `0.106.0`, and subsequent task commits use
successive patch versions in `Cargo.toml` and `Cargo.lock`. The migration boundary
stays `0.106.0`; the actual final package version is the last committed patch.
Do not create a planning-only version bump before design review. Commit the
reviewed documents with the first implementation commit.

## File and interface map

| New area | Responsibility |
| --- | --- |
| `src/skills/compatibility/{mod,hooks,names,projection,provenance}.rs` | Pure legacy/canonical name and extension projection; reversible source provenance |
| `src/workspace/instructions/{mod,document,legacy,render,tests}.rs` | Managed-region parsing, exact legacy adoption, common-contract render, safe document plans |
| `src/response_policy/{mod,render,tests}.rs` | Structured channel policy, common limit, shared template rendering |
| `templates/workspace/response/{interactive,sms,email}.md` | Single authored source of channel presentation rules |
| `src/agent/instructions.rs` | Compose bounded workspace guidance and trusted launch policy |
| `src/skills/install_transaction/package.rs` | Typed complete workspace package and narrow patch inventory |
| `src/startup_migration/brain_skills.rs` | Version boundary delegating to the shared package coordinator |
| `skills/{projects,process-capture,organize,brain-sync,agenda}/` | Focused workflow entrypoints and references |
| `tests/brain_skill_contract/` | Content ownership, references, aliases, and rendered scenario fixtures |
| `tests/brain_skill_migration/` | Upgrade, source projection, ownership, recovery, downgrade, and sync acceptance |

Retain focused existing modules rather than putting this logic in `main.rs`,
`mod.rs`, or one giant migration file. New test suites are split by behavior.
Existing transaction APIs may remain as wrappers for skill-only tests; production
workspace reconciliation must use the complete package path.

## Task 1: Define compatibility names, hooks, and provenance

**Files:** Create `src/skills/compatibility/{mod,hooks,names,projection,provenance}.rs`
and focused sibling tests. Modify `src/skills/mod.rs`, `src/skills/extension.rs`.
Update `docs/decisions.md` with compatibility semantics.

**Interfaces:** Consume raw extension text plus its relative source identity.
Produce these pure APIs; `Extension` is the existing parsed model:

```rust
pub(crate) enum ProjectionDirection { Current, Legacy105, Legacy101 }
pub(crate) struct ExtensionProjection {
    pub by_skill: std::collections::BTreeMap<String, crate::skills::extension::Extension>,
    pub workspace: crate::skills::extension::Extension,
    pub legacy_task_context: String,
    pub legacy_source_patches: Vec<SourcePatch>,
}
pub(crate) struct SourcePatch {
    pub relative_path: std::path::PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
    pub provenance: ProjectionProvenance,
}
pub(crate) struct ProjectionProvenance {
    pub source_path: String,
    pub source_sha256: String,
    pub logical_hook: String,
    pub inserted_sha256: String,
}
pub(crate) fn project_extensions(
    sources: &std::collections::BTreeMap<String, String>,
    direction: ProjectionDirection,
) -> anyhow::Result<ExtensionProjection>;
pub(crate) fn expand_legacy_names(names: &[String]) -> Vec<String>;
```

- [ ] Write a failing pure test using one legacy agenda hook and one task hook:

```rust
#[test]
fn legacy_hooks_reach_only_their_semantic_owner() {
    let sources = std::collections::BTreeMap::from([(
        "todo.md".to_owned(),
        "[todo:anchors]\nANCHOR\n[todo:linear]\nTRACKER\n".to_owned(),
    )]);
    let projected = project_extensions(&sources, ProjectionDirection::Current).unwrap();
    assert_eq!(projected.by_skill["agenda"].hooks["agenda:anchors"], "ANCHOR");
    assert_eq!(projected.by_skill["todo"].hooks["todo:linear"], "TRACKER");
    assert!(!projected.by_skill["todo"].hooks.contains_key("todo:anchors"));
    assert!(projected.legacy_source_patches.is_empty());
}
```

- [ ] Run `cargo test --release skills::compatibility::` and observe the missing
  implementation failure. Implement the exact hook map from spec section 3.
- [ ] Add successive red/green cases for all 16 hooks, empty sources, catch-all,
  unknown hooks, identical definitions, conflicting definitions, and duplicate
  section headers. Checked parsing must reject duplicate-loss ambiguity before
  the existing `BTreeMap` parser can overwrite a block.
- [ ] Preserve raw source bytes and origin alongside parsed content. Do not
  normalize or rewrite arbitrary plugin prose. Preserve original bases for moved
  extension resource links and leave opaque shell fragments unchanged.
- [ ] Test name expansion with `todo`, `second-brain`, an unrelated custom name,
  and an empty list. Input names use the existing validated normalized spelling;
  append successors deterministically and deduplicate without granting unrelated names.
- [ ] Define down projection and provenance now, with tests that differing later
  edits conflict rather than being replaced by an old backup. Source patches are
  planned, not written, by this module. Re-run the focused suite and commit.

## Task 2: Centralize reply policy and deterministic rendering

**Files:** Create `src/response_policy/{mod,render,tests}.rs` and response templates.
Modify `src/lib.rs` / module declarations as appropriate, `src/server/reply/mod.rs`,
and the email body construction seam in
`src/state/receiver/model/delivery/envelope.rs`.
Keep HTML/plain-text helpers in `src/server/reply/`. Update `docs/integrations.md`.

**Interfaces:** `policy(Channel) -> ResponsePolicy`,
`render_instructions(Channel) -> String`, `render_workspace_reference() -> String`.
`ResponsePolicy` contains channel, optional character limit, Markdown allowance,
local-path allowance, and changes-table allowance. `SMS_LIMIT` has one owner.

- [ ] Write and run the failing structured policy test:

```rust
#[test]
fn sms_policy_is_selected_from_channel_not_request_text() {
    let sms = policy(crate::actor::Channel::Sms);
    assert_eq!(sms.max_characters, Some(480));
    assert!(!sms.markdown);
    assert!(!sms.local_paths);
    assert!(!sms.changes_table);
    assert!(policy(crate::actor::Channel::Interactive).local_paths);
    assert!(policy(crate::actor::Channel::Email).markdown);
}
```

- [ ] Implement pure selection and authored templates. Generate the all-channel
  workspace reference and selected launch text from these same inputs. Do not
  duplicate a prose policy in skills or adapter implementations.
- [ ] Move the SMS limit authority, retaining `server::reply::SMS_LIMIT` as a
  re-export if useful. Preserve strip-before-count semantics and UTF-8 safety.
  Add a failing long-reply test asserting the fallback does not promise email
  without confirmed availability. Use a capability-neutral fallback if the
  current reply envelope does not carry that availability.
- [ ] Test email construction with Markdown tables, Unicode, safe web URLs,
  unsafe HTML, and a readable plain-text alternative. Inspect the existing typed
  delivery envelope before changing its body builder; do not add a second send.
- [ ] Verify unchanged recipient/thread/idempotency behavior through existing
  delivery-policy tests. Run `cargo test --release response_policy::`,
  `cargo test --release server::reply::`, and
  `cargo test --release state::receiver::tests::delivery_policy`. Commit.

## Task 3: Model managed workspace instructions without filesystem mutation

**Files:** Create `src/workspace/instructions/{mod,document,legacy,render,tests}.rs`.
Freeze the exact baseline template in a narrowly scoped legacy asset. Modify
`src/workspace/mod.rs` and `templates/workspace/AGENTS.md`. Update `docs/config.md`.

**Interfaces:**

```rust
pub(crate) struct InstructionReceipt {
    pub schema_version: u32,
    pub generated_revision: u32,
    pub body_sha256: String,
    pub sources_sha256: String,
}
pub(crate) struct DocumentPlan {
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
    pub receipt: InstructionReceipt,
    pub changed: bool,
}
pub(crate) fn plan_document(
    existing: Option<&[u8]>,
    desired_body: &str,
    prior: Option<&InstructionReceipt>,
) -> anyhow::Result<DocumentPlan>;
```

- [ ] Add the smallest failing test preserving custom text around a managed region:

```rust
#[test]
fn user_text_outside_owned_region_survives() {
    let old = b"# Personal\nKeep this exactly.\n";
    let first = plan_document(Some(old), "Core one", None).unwrap();
    let next = plan_document(Some(&first.after), "Core two", Some(&first.receipt)).unwrap();
    assert!(next.after.starts_with(old));
    assert_eq!(next.before.as_deref(), Some(first.after.as_slice()));
    assert!(!String::from_utf8(next.after).unwrap().contains("Core one"));
}
```

- [ ] Run `cargo test --release workspace::instructions::`. Implement exact
  delimiters and lossless prefix/suffix preservation, with separately hashed
  managed content. Do not use heading-name matching as ownership proof.
- [ ] Add red/green tests for absent document, exact legacy template, exact unique
  legacy sections in custom text, partially edited legacy sections, invalid UTF-8,
  duplicate/malformed markers, removed region, modified region, and unknown newer
  revision. Receipt absence/staleness can adopt only a complete known render.
- [ ] Render the concise shared contract from spec section 1 plus Task 2's policy.
  Include conditional canonical/legacy routes and the precise authorization,
  naming, media/notes, and native-data decisions. Do not embed private content in
  the legacy asset or new core beyond the already-generic historical template.
- [ ] Test that a second identical plan is unchanged and a user suffix edit alone
  does not invalidate region ownership. Add legacy removed-span provenance needed
  by down. Run focused tests and commit.

## Task 4: Publish skills and instructions as one recoverable package

**Files:** Create `src/skills/install_transaction/package.rs` and focused package
tests. Modify `install_transaction.rs`, its `ownership`, `validation`, `journal`,
`snapshot`, `inspection`, `disposal`, `legacy` modules, and `skills/receipt.rs`.
Update `docs/architecture.md`, `docs/data-model.md`, and `docs/testing.md`.

**Interfaces:** Keep `RenderedInstall` for skill maps. Introduce:

```rust
pub(crate) struct ManagedWorkspacePackage {
    pub skills: RenderedInstall,
    pub instructions: Option<crate::workspace::instructions::DocumentPlan>,
    pub config_patches: Vec<JsonFieldPatch>,
    pub source_patches: Vec<crate::skills::compatibility::SourcePatch>,
}
pub(crate) struct JsonFieldPatch {
    pub relative_path: std::path::PathBuf,
    pub field: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}
```

`LockedInstall::preflight_package`, `install_package`, and a test-only
`install_package_with` mirror existing preflight/install/checkpoint seams.
Only `AGENTS.md`, the `allowed_skills` config field, and explicitly mapped
legacy extension source paths are admitted as additional artifact kinds.

- [ ] Write a failing temporary-root test: install old skills plus old instructions;
  interrupt after publishing the first new artifact; recover; assert **both** old
  instruction bytes and old skill/reference bytes, custom extras, and frontend links.
  Use `InstallIdentity::sandbox_with_cache`, `NoStamp`, and existing `Checkpoint`
  conventions from `src/skills/transaction_tests.rs`.
- [ ] Run `cargo test --release skills::install_transaction::`. Add typed entries and schema-2
  manifest/journal readers while retaining schema-1 decoding and recovery. Validate
  exact path kinds, roots, UUID, before/after fingerprints, and bounded state reads.
- [ ] Add red/green cases at staged, backed-up, installed, manifest-published,
  stamp-failed, and disposal boundaries. A pre-commit interruption restores all
  artifacts; a post-commit interruption finishes the same committed generation.
- [ ] Own only the AGENTS region for currentness but compare the entire before-file
  during replacement. Preserve modes, unowned text, user skill extras, and source
  before-images. No-follow errors and mid-plan edits must retain evidence.
- [ ] Test package rejection for a forged arbitrary path, external AGENTS symlink,
  foreign frontend entry, modified managed region, and unexpected source edits.
  Prove no live changes occur when preflight finds a conflict.
- [ ] Export a schema-1 skill-only manifest for a legacy target after successful
  down. Do not leave schema-2 journals for an older binary. Run all existing
  `skills::transaction` suites plus package tests, then commit.

## Task 5: Share checked extension projection across all render paths

**Files:** Modify `src/skills/{install,render,model}.rs`, capability rendering in
`src/access/skills.rs`, and tests. Add a focused source-snapshot helper under
`src/skills/compatibility/`. Update `docs/features.md` and `docs/decisions.md`.

**Interfaces:** A checked source snapshot feeds Task 1's projection. Ordinary,
selected-capability, and installer renderers consume that projection. The
shared legacy-task reference is a logical resource with one content digest,
packaged within each selected consumer rather than depending on an unselected skill.

- [ ] Add a failing integration test with legacy `todo.md` anchors and mixed
  catch-all text, rendered once normally and once for an agenda-only selection.
  Assert the same extension content, self-contained references, one hook placement,
  and byte-identical unchanged source files.
- [ ] Run `cargo test --release skills::install`. Replace the selected renderer's
  unchecked `extension::load` path with the shared checked resolver. Retain exact
  machine-source validation and capability restrictions.
- [ ] Render the common read-AGENTS prelude into installed Brain skills without
  channel branches. Preserve YAML frontmatter, plugin scripts/assets, and relative
  asset bases. Source files remain untouched on normal render.
- [ ] Test unreadable/invalid/symlinked sources, duplicate hooks, canonical/legacy
  collisions, unknown blocks, empty defaults, and plugin binary assets. Do not
  accept missing customization as a successful empty extension.
- [ ] Add a test for two handoff consumers sharing the same legacy context ID;
  the content instructs reuse without rerunning completed side effects. Record
  behavioral verification for this case in Task 12 rather than claiming a string
  count proves agent behavior. Run focused rendering tests and commit.

## Task 6: Extract focused knowledge workflows and compatibility routes

**Files:** Create `skills/projects/`, `process-capture/`, `organize/`, `brain-sync/`
with only necessary references. Rewrite `skills/second-brain/SKILL.md` as a compact
compatibility router. Update contacts, knowledge-capture, article-summarizer,
brain-skills, and their links. Add `tests/brain_skill_contract.rs` and focused
modules under `tests/brain_skill_contract/`. Update features/architecture docs.

**Interfaces:** Skill names, ownership, and aliases are the spec section 1 table.
Canonical project/task linkage is `skills/projects/references/task-project-link.md`;
the old todo reference path forwards to it. Organize owns retrieval and local
reindex; brain-sync owns cloud sync and conflict resolution.

- [ ] Write failing catalog/link tests for the four new skills and compatibility
  targets. Build a structural walker over bundled Markdown links and explicit
  slash routes, excluding fenced examples and documented sample placeholders.
  Validate real relative file targets and heading anchors in source and rendered
  output. Do not lock entire documents to exact prose.
- [ ] Run `cargo test --release --test brain_skill_contract`. Move each behavior
  inventoried in the review to its owner. Preserve native project/removal commands,
  IP completion gates, capture accounting, note/media indexing, retrieval scope,
  and sync partial-result reporting.
- [ ] Preserve all supported old anchors as small forwarding sections. Update
  first-party links to canonical paths; retain old task-project links for plugins.
  Do not preserve obsolete command examples that contradict current native APIs.
- [ ] Convert final-output demands in contacts/article/knowledge skills into
  semantic content requirements. Saved-summary headings remain stable. Remove
  duplicated persona/channel/cleanup policy now owned by the workspace contract.
- [ ] Document `.config/plugins` as the preferred authoring source and preserve
  existing unmarked user skill directories. Keep the bundled core extension-neutral.
- [ ] Run the existing `skills::embed` and installed-command-example suites with
  deliberate updated contracts, plus link tests. Commit the knowledge split.

## Task 7: Extract agenda planning and update its callers

**Files:** Create `skills/agenda/SKILL.md` and focused planning/artifact references.
Reduce `skills/todo/SKILL.md` and its references to task/habit ownership. Update
`skills/triage/SKILL.md`, its references, `src/tui/app_actions/commands.rs`, and
the matching prompt tests. Update `docs/{features,keybindings,integrations}.md`.

**Interfaces:** `/agenda` is canonical for plan generation/update. `/todo agenda`
and `/todo plan-day` route to it. `/todo today`, `what`, `next`, and counts do not.
Expose a pure `generate_agenda_prompt() -> &'static str` and call it from the
existing palette action; it names `/agenda` and configured output resolution.

- [ ] Add a failing routing/prompt test that agenda generation delegates to agenda
  and does not hardcode a Markdown directory. Add contract fixtures distinguishing
  task listing, task recommendation, first agenda build, routine update, native
  mutation synchronization, and explicit regeneration.
- [ ] Run `cargo test --release --test brain_skill_contract` and the focused
  `tui::app_actions` tests. Extract scheduling, date/capacity rules, chunk planning,
  artifact structure, PDF constraints, and post-build hook into agenda references.
- [ ] Use `brain env get agenda_markdown_dir` and the existing PDF-directory
  setting to resolve destinations; verify exact CLI names against current schema
  before writing examples. Native mutation sync remains owned by Rust.
- [ ] Remove personal schedules and fixed habit IDs from generic core. Preserve
  configured habit `ideal_time`, recurrence and explicit anchors/calendar/cutoff.
  Empty extensions create no named habits, quotas, or personal timetable.
- [ ] Correct managed-triage skip to use the keyed helper and make triage delegate
  capture/agenda only when needed. Preserve all seven triage hook phases and runtime
  completion `require` data. Generic triage does not assume an agenda/PDF extension.
- [ ] Repair moved stale references and command maps using native behavior. Test
  configured agenda directories and existing PDF refresh behavior through existing
  agenda IO/sync tests; do not rewrite the scheduler or introduce duplicate CRUD.
- [ ] Verify `/todo` task-only fixtures do not depend on reading agenda references.
  Run focused tests and commit.

## Task 8: Deliver shared instructions through AgentController

**Files:** Create `src/agent/instructions.rs` and focused tests. Modify
`src/agent/{frontend,controller/mod,claude,codex,opencode,pi}.rs`, adapter test
modules, and receiver prompt budgeting if required. Update `docs/integrations.md`
and `docs/adding-an-agent-frontend.md`.

**Interfaces:** `LaunchInstructions` has separate workspace-guidance and trusted
policy text. A pure composer consumes reconciled text, `ActorContext`, and
`AccessPolicy`; it never derives the channel from body text. Controller preparation
loads the document before adapter/spawn side effects. `LaunchRequest` carries the
prepared payload through a dedicated accessor; access policy keeps its existing role.

- [ ] Add a failing controller test recording the prepared instruction payload for
  an unrestricted SMS actor even when workspace text says `Channel: email`.
  Assert the resolved channel policy stays SMS and existing capability boundaries
  remain present. A preparation failure produces no spawn event.
- [ ] Run `cargo test --release agent::controller::`. Implement the loader/composer
  and wire all four adapters to the common payload. Preserve command prefixes,
  shell quoting, hook metadata, working directory, and lifecycle identities.
- [ ] Add fresh/resume tests across `AgentKind::ALL`, all three channels, and both
  access modes. Inspect actual command/config arguments, not only a mock
  controller property. Use the existing adapter fixtures and local fake executables.
- [ ] Extend tests to receiver portable-transcript reconstruction, recovery-only
  launch, main/manual tabs, and skill sessions. Existing input, completion, and
  `require` protocols remain byte-compatible where not changed by added policy.
- [ ] Reserve instruction overhead in the 96 KiB command budget. Add adversarial
  quote/Unicode/large-instruction cases and retain bounded whole attachment paths.
  Refuse oversized rules before spawn; do not silently truncate them.
- [ ] Run adapter/controller/receiver-planning suites and commit. Do not claim
  native pi AGENTS loading as the proof: assert explicit payload delivery.

## Task 9: Migrate capability selections without changing authority

**Files:** Modify `src/config.rs`, `src/settings/schema.rs`,
`src/access/skills/capability_plan.rs`, `src/skills/compatibility/names.rs`, and
capability status/selection tests. Update `docs/config.md` and `docs/data-model.md`.

**Interfaces:** Build `JsonFieldPatch` for explicit `allowed_skills` values; a
missing field uses new defaults without being materialized. Reverse mapping
consumes recorded before/after values and returns an error for unrepresentable scope.

- [ ] Add failing pure tests for legacy defaults, todo-only, second-brain-only,
  mixed canonical/legacy, empty, custom, and already-migrated lists.

```rust
#[test]
fn decomposition_preserves_unrelated_selection_and_empty_denial() {
    assert!(expand_legacy_names(&[]).is_empty());
    let selected = vec!["todo".to_owned(), "custom-skill".to_owned()];
    assert_eq!(expand_legacy_names(&selected), vec!["todo", "custom-skill", "agenda"]);
}
```

- [ ] Run `cargo test --release --test workspace_access_policy` and compatibility
  name tests. Implement narrow config projection, recording exactly the introduced
  entries. Preserve unrelated JSON values and original configured capabilities.
- [ ] Preflight new bundled names against plugin sources, unmarked user trees,
  frontend destinations, and machine skill selections. Bundled-first resolution
  must not silently replace an existing machine-owned `agenda` or `projects`.
- [ ] Test canonical-only `projects` down to an old umbrella: refuse instead of
  granting organize/sync/capture too. Equivalent legacy-derived lists can reverse;
  later user additions survive or produce an explicit conflict.
- [ ] Verify every available alias route reaches an allowed successor; unavailable
  delegated workflows produce a clear capability result. Do not automatically
  grant all skills just because triage can compose them. Run focused tests and commit.

## Task 10: Integrate automatic package reconciliation and sync arrival

**Files:** Create `src/startup_migration/brain_skills.rs`; modify
`src/startup_migration/{mod,task_deletion_skills}.rs`,
`src/skills/{reconcile,repair,install}.rs`, `src/workspace/templates.rs`,
`src/workspace/initialize/seed.rs`, setup/launch seams, `src/sync/journal.rs`,
`src/sync/command/`, and narrow startup recovery routing in `src/main.rs`.
Add `tests/brain_skill_migration.rs` plus `startup`, `instructions`,
`capabilities`, and `sync_arrival` modules. Update architecture/features/testing docs.

**Interfaces:** One `RefreshAttempt` represents the complete workspace package.
The existing historical callback calls compatibility preparation before its first
render. The new version entry delegates up/down to this same coordinator.
Package revision is `2`; no existing portable workspace version floor changes.
Persist a UUID-scoped receive-in-progress state before incoming portable writes;
clear it after receive completion and successful package reconciliation. The
existing completed-run journal is not sufficient evidence of a pending receive.

- [ ] Write failing acceptance tests for upgrade from before `0.102.25` and from
  `0.105.3`, proving preparation occurs before the new renderer and only one
  refresh attempt occurs. Use a legacy extension and explicit old allowlist.
- [ ] Run `cargo test --release --test brain_skill_migration`. Implement the shared
  package path for normal/explicit/all-root/sandbox calls. Stamp only after all
  instructions, skills, and narrow config patches verify successfully.
- [ ] Add red/green tests for equal-version repair, patch upgrade, disabled
  `skills_auto_sync`, missing default root, another failed root, remount, malformed
  identity, and an earlier historical migration failure. Preserve aggregate
  reporting and do not manufacture absent roots.
- [ ] Preserve empty-root seeding. Test absent AGENTS, recognized generated-only
  AGENTS, arbitrary existing prose, and initial pull before PARA creation. A
  generated instruction file alone must not suppress native seeding.
- [ ] Acquire the sync boundary before managed installation when portable files
  change; retain existing operation/task lock order where applicable. Test live
  lock contention and concurrent changed before-images using deterministic barriers.
- [ ] After a completed verified pull, reconcile a received supported region with
  missing **or stale** receipt. Test rules arriving before their extension source,
  sources arriving first, and an unknown newer region. Defer only while the pull
  is incomplete; preserve evidence for a mismatch after completion.
- [ ] Add a crash/restart test: stop after receiving AGENTS but before its matching
  extension, then invoke `brain sync --pull` for that registered root. Implement
  a recovery-only startup continuation that completes one verified pull under
  the sync lock before package reconciliation. Run required prerequisites first;
  block ordinary dispatch, agent launch and push until reconciliation succeeds.
  A failed recovery remains retryable. Test that a modified document without
  interrupted-receive evidence does not gain an overwrite bypass, and a mismatch
  after a completed receive becomes a reported conflict without infinite retries.
- [ ] Keep the current no-write fast path and optional-customization gate for
  unchanged packages. Explicit sync sees changed sources. Help/version tests must
  verify zero workspace writes. Run focused startup/setup/sync suites and commit.

## Task 11: Make installer preflight and downgrade understand the full package

Execution clarification from the observed replacement-window regression: extend
the existing Rust helper to own migration, typed apply, digest revalidation and
atomic replacement under one exclusive machine invocation guard. Both upgrade
and downgrade migrate/apply before replacement. Ordinary main retains shared
ownership through dispatch/TUI lifetime, refuses contention immediately, and checks
compiled versus on-disk executable version after acquisition. Help/version precede
the guard; hidden maintenance acquires ownership too. Conservatively refuse live
historical TUI/server evidence without terminating processes or removing evidence.
Preserve sync -> operation/task -> install ordering without reentrant acquisition.
Protected finalization starts at 0.106.14; reject modern-split 0.106.0-0.106.13
downgrade helpers before live mutation with an exact current-installer-first remedy.
Historical pre-split helpers retain historical behavior; uninstrumented historical
ordinary CLI processes remain limited to existing workspace locks. Add deterministic
RED/GREEN coverage for the original contender at the actual Rust replacement
boundary, active/queued commands, stale loaded image policy, migration/apply failure,
help/version, candidate changes and interruption/retry. Use a cfg(test) seam rather
than production test flags. No persistent ticket, new installer, dependency or unsafe.

**Files:** Modify `src/startup_migration/skill_refresh/{target,sources,replica}.rs`,
`src/startup_migration/skill_refresh.rs`, `install.sh`, and Task 1 provenance logic.
Extend `tests/task_deletion_skill_refresh/` and add focused `installer`, `downgrade`,
and `source_projection` modules under `tests/brain_skill_migration/`.
Update `docs/integrations.md` and `docs/decisions.md`.

**Interfaces:** Explicit target profiles describe `0.101.0`, `0.105.3`, and the
exact current binary, including managed manifest schema and compatibility revision.
Private replicas receive projected instructions, source patches, config, and
provenance; their output is a typed `ManagedWorkspacePackage`.

- [ ] Write a failing preflight test where a canonical `agenda:anchors` source
  must appear as `todo:anchors` in old target output before any live source edit.
  Prove target source snapshots and live source snapshots are distinct.
- [ ] Run the focused installer suite. Implement exact profiles and remove the
  hardcoded nonlegacy revision `1`. Keep digest-bound target execution and no-follow
  private snapshots. Cache only customization-free core inventory between roots.
- [ ] Validate target output using the shared projection rules rather than the old
  same-basename-extension formula. Include AGENTS/config/provenance in before-image
  revalidation. Preflight retains existing recovery side effects where required;
  it does not mutate live customization to make the candidate render.
- [ ] Persist down projections into the legacy source files with before-images
  and provenance, alongside compatible instructions and skill output. Later old
  ordinary `skills sync` must retain those customizations. Keep canonical new files.
- [ ] Test old-client edits after down, re-upgrade deduplication, identical mixed
  sources, conflicting mixed sources, unrelated config edits, foreign files,
  target-incompatible workspace floor, and unrepresentable capability scope.
- [ ] Reject down before executable replacement if any affected registered root
  is unavailable. Do not leave work for an older binary that cannot run this
  migration. Upgrades still retain pending roots for automatic later repair.
- [ ] Run actual audited target executables in isolated HOME/XDG/TMP fixtures for
  upgrade, down, later ordinary sync, and re-upgrade. Reuse the existing historical
  binary fixture/build mechanism; never install a test target into the real bin dir.
- [ ] Test interruption around instruction/skill/config/source publication,
  manifest lowering, receipt stamping, and executable replacement. Verify old
  ordinary startup reads the resulting manifest schema. Run shell syntax checks
  and focused installer tests; commit.

## Task 12: Behavioral review, documentation, and release verification

**Files:** Complete contract/migration suites and generic scenario fixtures.
Update `docs/{architecture,features,integrations,config,data-model,decisions,testing,
glossary,keybindings}.md` as required by actual changes. Keep the spec and plan
consistent with final implementation decisions.

**Interfaces:** The release evidence is a scenario/result matrix plus exact test
commands and commits. Separate automated guarantees from agent-behavior review.

- [ ] Run the complete structural link/anchor/alias validator on bundled and
  rendered variants: no customization, legacy customization, canonical
  customization, mixed compatible customization, and representative plugins.
- [ ] Independently forward-test rendered skills with isolated synthetic artifacts.
  Give reviewers requests and raw fixtures, not intended answers. Cover contact
  lookup, task listing, agenda generation, task mutation after agenda, mixed
  capture, project completion/IPs, sync conflict partial failure, and triage with
  absent/present required outputs. No real service credentials or live delivery.
- [ ] Repeat representative replies across interactive/SMS/email and all four
  launch adapters. Verify files retain artifact formatting while the final reply
  follows central policy. A skill must not force an SMS table or local path.
- [ ] Resolve review findings through a scoped red/green change and re-review.
  In particular, inspect opaque legacy catch-all handoffs for duplicate side effects,
  canonical-only capability denial, and user rules preserved during down/re-upgrade.
- [ ] Update each relevant product doc with behavior and rationale, not merely
  new filenames. Document managed ownership, extension aliases, collisions,
  supported target profiles, pending-root semantics, and conditional legacy routing.
- [ ] Run required final checks once on the final changed code:

```sh
cargo fmt --check
cargo test --release
cargo clippy --release --all-targets -- -D warnings
sh -n install.sh
git diff --check
```

- [ ] Run the repository's existing personal-data and bundled-command guards from
  the full test suite; inspect failure output rather than suppressing it. Repeat
  only covering checks after any subsequent fix, plus required final gates as needed.
- [ ] Verify final diff, version/lock agreement, docs, review findings, and clean
  temporary state. Record actual test outcomes and material limits. Commit the
  final changes, then follow session authorization for any merge/push/install.
  Do not remove the worktree before requested/authorized integration is verified.

## Spec coverage

| Spec obligation | Plan tasks |
| --- | --- |
| Shared concise contract and managed ownership | 3, 4, 10 |
| Focused knowledge skills and no mandatory umbrella | 5, 6 |
| Agenda/task separation and configured artifacts | 7 |
| Central channel policy and artifact/reply distinction | 2, 6, 7, 8, 12 |
| All frontend, access, resume, and recovery paths | 5, 8, 9, 12 |
| Every legacy hook, catch-all, plugin path and alias | 1, 5, 6, 7, 11, 12 |
| Capability equivalence and collision protection | 1, 4, 9, 11 |
| Startup, all roots, missing roots, seeding, currentness | 4, 10 |
| Sync arrival without a false local release fence | 3, 10 |
| Reversible down, source persistence, target profiles | 1, 4, 9, 11 |
| Failure atomicity and protected user edits | 3, 4, 10, 11 |
| Native behavior, no personal defaults, meaningful tests | 6, 7, 12 |
| Product documentation and release checks | Every owning task; final gate 12 |
