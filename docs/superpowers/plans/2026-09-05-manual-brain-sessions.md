# Manual Brain Sessions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add named, persistent manual brain sessions in additional tabs while keeping Main fixed at tab 0 and preserving skill-session and receiver-session behavior.

**Architecture:** Keep Main structurally separate and refactor the existing additional-tab collection into typed Manual, Skill, and Receiver entries. Persist only manual-session identities and order in SQLite, drive every live agent through `AgentController`, and expose stable tab-ID actions through both command-palette surfaces.

**Tech Stack:** Rust 2024, rusqlite, ratatui, crossterm, Python lifecycle bridge, Cargo release tests and Clippy.

## Global Constraints

- Follow RED, GREEN, REFACTOR for every behavior; record the expected failing assertion before production edits.
- Main is always a manual session at tab 0 and is never user-closeable.
- Additional manual sessions persist across orderly shutdown and restart.
- Skill sessions remain ephemeral and absent from `brain_sessions` and `manual_sessions`.
- Receiver sessions remain receiver-owned, background-safe, and absent from user Close commands.
- Every agent launch and lifecycle path must work through `AgentController` for Claude, Codex, and OpenCode.
- Do not add dependencies and do not add `unsafe`.
- Preserve stable `SessionTabId` selection when neighboring tabs close.
- Update every documentation file required by the repository docs contract.
- Keep files focused; split production or test files that cross the repository's approximate 400-line review threshold.
- Use one final implementation commit so the repository's version-per-commit rule produces one release bump from `0.86.15` to `0.87.0`.

---

### Task 1: Manual-session domain model and durable store

**Files:**

- Create: `src/manual_session/mod.rs`
- Create: `src/manual_session/tests.rs`
- Create: `src/state/manual_session/mod.rs`
- Create: `src/state/manual_session/schema.rs`
- Create: `src/state/manual_session/store.rs`
- Create: `src/state/manual_session/tests.rs`
- Modify: `src/lib.rs`
- Modify: `src/state/mod.rs`
- Modify: `src/state/database.rs`

**Interfaces:**

- Produces: `ManualSessionId`, `ManualSessionName`, `ManualSessionNameError`, `ManualSessionRole`, and `ManualSessionRecord`.
- Produces: `Db::manual_sessions`, `Db::register_fresh_manual_session`, `Db::attach_manual_session`, `Db::replace_manual_session`, `Db::close_manual_session`, and `Db::release_manual_session`.
- Consumes: existing `AgentSession`, `SessionScope`, `brain_sessions`, and the state database migration sequence.

Use these exact store signatures:

```rust
pub(crate) fn manual_sessions(&self, scope: &SessionScope) -> anyhow::Result<Vec<ManualSessionRecord>>;
pub(crate) fn register_fresh_manual_session(&self, record: &ManualSessionRecord, pid: i32, scope: &SessionScope) -> anyhow::Result<()>;
pub(crate) fn attach_manual_session(&self, record: &ManualSessionRecord, scope: &SessionScope) -> anyhow::Result<()>;
pub(crate) fn replace_manual_session(&self, id: &ManualSessionId, session: &AgentSession, pid: i32, scope: &SessionScope) -> anyhow::Result<ManualSessionRecord>;
pub(crate) fn close_manual_session(&self, id: &ManualSessionId, scope: &SessionScope) -> anyhow::Result<()>;
pub(crate) fn release_manual_session(&self, id: &ManualSessionId, scope: &SessionScope) -> anyhow::Result<()>;
```

- [x] **Step 1: Write failing domain tests for normalized unique names**

Add literal behavior tests to `src/manual_session/tests.rs`:

```rust
#[test]
fn a_manual_session_name_is_trimmed_before_storage() {
    let name = ManualSessionName::parse("  Project Atlas  ", &[]).unwrap();
    assert_eq!(name.as_str(), "Project Atlas");
}

#[test]
fn blank_and_case_insensitive_duplicate_names_are_rejected() {
    assert_eq!(
        ManualSessionName::parse("   ", &[]),
        Err(ManualSessionNameError::Blank)
    );
    assert_eq!(
        ManualSessionName::parse(" brain ", &["Brain".to_owned()]),
        Err(ManualSessionNameError::Duplicate)
    );
}
```

- [x] **Step 2: Run the domain tests and verify RED**

Run: `cargo test --release manual_session::tests::`

Expected: compilation fails because `manual_session` and its types do not exist.

- [x] **Step 3: Implement the typed domain model**

Add the exact public shape in `src/manual_session/mod.rs` and export the module from `src/lib.rs`:

```rust
pub const MAIN_SESSION_TITLE: &str = "Brain";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ManualSessionId(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualSessionName(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualSessionRole {
    Main,
    Additional,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualSessionRecord {
    pub id: ManualSessionId,
    pub agent_session: crate::agent::AgentSession,
    pub name: ManualSessionName,
    pub position: u32,
    pub role: ManualSessionRole,
}
```

Implement `ManualSessionId::{new, parse, as_str}`, `ManualSessionName::{parse, main, as_str}`, `ManualSessionRecord::{main, additional, id, agent_session, name, position, role}`, and `ManualSessionRole::{as_str, parse}`. `ManualSessionName::parse` must trim once and compare with `eq_ignore_ascii_case` against every supplied open title.

- [x] **Step 4: Run the domain tests and verify GREEN**

Run: `cargo test --release manual_session::tests::`

Expected: all manual-session domain tests pass.

- [x] **Step 5: Write failing migration and store tests**

Add state tests that exercise the real in-memory SQLite database:

```rust
#[test]
fn manual_sessions_round_trip_in_main_then_position_order() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main = record("main-id", "main-native", "Brain", 0, ManualSessionRole::Main);
    let atlas = record("atlas-id", "atlas-native", "Atlas", 1, ManualSessionRole::Additional);

    db.register_fresh_manual_session(&atlas, 42, &scope).unwrap();
    db.register_fresh_manual_session(&main, 42, &scope).unwrap();

    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![main, atlas]);
}

#[test]
fn closing_one_manual_session_releases_only_its_lock_and_compacts_positions() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main = record("main-id", "main-native", "Brain", 0, ManualSessionRole::Main);
    let first = record("first-id", "first-native", "First", 1, ManualSessionRole::Additional);
    let second = record("second-id", "second-native", "Second", 2, ManualSessionRole::Additional);
    for session in [&main, &first, &second] {
        db.register_fresh_manual_session(session, 42, &scope).unwrap();
    }

    db.close_manual_session(first.id(), &scope).unwrap();

    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![main, record("second-id", "second-native", "Second", 1, ManualSessionRole::Additional)]);
    assert_eq!(db.locked_session_for_instance("first-id", &scope), None);
    assert_eq!(db.locked_session_for_instance("second-id", &scope), Some("second-native".to_owned()));
}
```

Also add tests proving a second Main, position 0 Additional, case-insensitive duplicate title, and cross-scope mutation are rejected; `replace_manual_session` updates the exact native ID; `release_manual_session` preserves the mapping; and the down migration drops `manual_sessions` while preserving `brain_sessions`.

- [x] **Step 6: Run the state tests and verify RED**

Run: `cargo test --release state::manual_session::tests::`

Expected: compilation fails because the schema and store APIs are missing.

- [x] **Step 7: Implement schema version 14 and exact store operations**

Create `manual_sessions` with this schema in `src/state/manual_session/schema.rs`:

```sql
CREATE TABLE manual_sessions (
  manual_session_id TEXT NOT NULL,
  agent_kind        TEXT NOT NULL CHECK (agent_kind IN ('claude', 'codex', 'opencode')),
  agent_session_id  TEXT NOT NULL,
  workspace_id      TEXT NOT NULL,
  actor_id          TEXT NOT NULL,
  channel           TEXT NOT NULL CHECK (channel = 'interactive'),
  title             TEXT NOT NULL COLLATE NOCASE,
  position          INTEGER NOT NULL CHECK (position >= 0),
  role              TEXT NOT NULL CHECK (role IN ('main', 'additional')),
  PRIMARY KEY (agent_kind, workspace_id, actor_id, channel, manual_session_id),
  UNIQUE (agent_kind, workspace_id, actor_id, channel, title),
  UNIQUE (agent_kind, workspace_id, actor_id, channel, position),
  CHECK ((role = 'main' AND position = 0) OR
         (role = 'additional' AND position > 0)),
  FOREIGN KEY (agent_kind, agent_session_id, workspace_id, actor_id, channel)
    REFERENCES brain_sessions(agent_kind, agent_session_id, workspace_id, actor_id, channel)
);
CREATE UNIQUE INDEX manual_sessions_one_main
  ON manual_sessions(agent_kind, workspace_id, actor_id, channel)
  WHERE role = 'main';
PRAGMA user_version = 14;
```

`up(connection, current_version)` must be idempotent and transactional. `down_path(path)` must acquire an immediate transaction, drop the table and index, set `user_version` back to 13, and leave `brain_sessions` untouched.

Implement store methods transactionally. `register_fresh_manual_session` inserts the `brain_sessions` row and mapping in one transaction. `attach_manual_session` inserts a mapping only after the exact native row is claimed by the matching manual-session identity. `replace_manual_session` registers the fresh native row, updates the mapping, and releases the prior native row in one transaction. `close_manual_session` deletes only an Additional mapping, releases its exact lock, and decrements later Additional positions. Main close returns a typed invariant error.

- [x] **Step 8: Run state and existing session-store tests and verify GREEN**

Run: `cargo test --release state::manual_session::tests:: state::tests_sections::session_store`

Expected: new store tests and existing lock/recency tests pass.

---

### Task 2: Refactor additional tabs into typed session tabs

**Files:**

- Create: `src/tui/state/brain/sessions.rs`
- Create: `src/tui/state/brain/sessions/tests.rs`
- Delete: `src/tui/state/brain/ephemeral.rs`
- Modify: `src/tui/state/brain.rs`
- Modify: `src/tui/state/brain/tests.rs`
- Modify: `src/tui/state/brain/exhausted_tab_ids.rs`
- Modify: `src/tui/model.rs`
- Modify: `src/tui/app_brain_tab.rs`
- Modify: `src/tui/shell.rs`
- Modify: `src/tui/draw/brain_panel.rs`
- Modify: `src/tui/draw/mod.rs`

**Interfaces:**

- Consumes: `ManualSessionId`, `ManualSessionRecord`, `AgentController`, `SkillSessionKey`, `ReceiverJobId`, and `SessionTabId`.
- Produces: `SessionTabs`, `SessionTabKind`, `SessionPaletteEntry`, `ManualSessionObservation`, and exact kind-gated insert/remove/query operations.
- Preserves: lifetime-monotonic tab IDs, insertion order, receiver reservation, controller lookup, and receiver identity checks.

Use these exact new collection-facing methods:

```rust
pub(crate) fn session_tab_ids(&self) -> Vec<SessionTabId>;
pub(crate) fn user_session_rows(&self) -> Vec<SessionPaletteEntry>;
pub(crate) fn manual_session_rows(&self) -> Vec<SessionPaletteEntry>;
pub(crate) fn add_manual_session(&mut self, record: ManualSessionRecord, controller: AgentController, resumed_session_id: Option<String>) -> Result<SessionTabId, ManualSessionTabIdExhausted>;
pub(crate) fn remove_manual_session(&mut self, id: SessionTabId) -> Option<RemovedManualSession>;
pub(crate) fn manual_session_id(&self, id: SessionTabId) -> Option<&ManualSessionId>;
pub(crate) fn is_manual_session_tab(&self, tab: BrainTab) -> bool;
pub(crate) fn is_skill_session_tab(&self, tab: BrainTab) -> bool;
pub(crate) fn is_receiver_session_tab(&self, tab: BrainTab) -> bool;
```

- [x] **Step 1: Perform a dry-run impact scan for the refactor**

Run:

```sh
rg -n 'EphemeralTab|EphemeralTabs|EphemeralTabMetadata|ephemeral_tab_ids|handle_skill_session_key|active_is_skill_session' src docs
```

Record every production, test, and documentation reference. The planned rename is `EphemeralTabs` to `SessionTabs`, `EphemeralTabMetadata` to `SessionTabKind`, and `ephemeral_tab_ids` to `session_tab_ids`. Do not edit yet.

- [x] **Step 2: Write failing typed-tab tests**

Add tests that mix all three additional-tab kinds:

```rust
#[test]
fn manual_skill_and_receiver_tabs_share_order_but_keep_distinct_close_authority() {
    let mut brain = brain_state();
    let manual = brain.add_manual_session(additional_record("manual-id", "native", "Atlas", 1), controller()).unwrap();
    let skill = brain.add_skill_session(SkillSessionKey::DailyTriage, "Daily triage".to_owned(), "token".to_owned(), controller()).unwrap();
    let receiver = brain.add_receiver_run(receiver_job_id(), "Receiver · SMS".to_owned(), "receiver-instance".to_owned(), controller()).unwrap();

    assert_eq!(brain.session_tab_ids(), [manual, skill, receiver]);
    assert_eq!(brain.user_session_rows(), [
        SessionPaletteEntry::new(manual, "Atlas"),
        SessionPaletteEntry::new(skill, "Daily triage"),
    ]);
    assert!(brain.remove_manual_session(receiver).is_none());
    assert!(brain.remove_skill_session(manual).is_none());
}
```

Add tests for manual allocation exhaustion, stable remaining IDs after manual removal, `tab_titles()` ordering, `is_manual_session_tab`, `is_skill_session_tab`, `is_receiver_session_tab`, and shutdown continuing across all kinds.

- [x] **Step 3: Run typed-tab tests and verify RED**

Run: `cargo test --release tui::state::brain::tests::`

Expected: compilation fails because manual tab metadata and the renamed APIs do not exist.

- [x] **Step 4: Apply the tab-collection refactor and add Manual metadata**

Use this kind model in `src/tui/state/brain/sessions.rs`:

```rust
enum SessionTabKind {
    Manual(ManualTabMetadata),
    Skill(SkillSessionMetadata),
    Receiver(ReceiverSessionMetadata),
}

struct ManualTabMetadata {
    id: crate::manual_session::ManualSessionId,
    resumed_session_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionPaletteEntry {
    pub(crate) id: SessionTabId,
    pub(crate) title: String,
}
```

Keep one shared `add` allocator and make every type-specific removal locate both the stable tab ID and expected metadata variant before removing or shutting down anything. Receiver reservation and exact cleanup logic must retain their current preconditions and outcomes.

Update comments and method names to say additional or session tab, not ephemeral, except where they describe skill-session lifetime. Change the brain-panel footer input from `active_is_skill_session` to `active_close_kind: Option<SessionCloseKind>`, where only Additional Manual and Skill render `^X close tab`.

- [x] **Step 5: Run state, tab navigation, drawing, skill, and receiver tests and verify GREEN**

Run:

```sh
cargo test --release tui::state::brain::
cargo test --release tui::state::shell::
cargo test --release tui::draw::brain_panel::
cargo test --release tui::app_brain::tests::skill_session
cargo test --release tui::app_brain::tests::receiver_tab
```

Expected: all pass with Main still slot 0, mixed tabs still in insertion order, and receiver behavior unchanged.

- [x] **Step 6: Repeat the refactor impact scan**

Run the Step 1 `rg` command again. Remaining uses of `ephemeral` must refer only to the intentional lifetime of skill sessions, not the shared collection or all additional tabs.

---

### Task 3: Shared manual-session launch, restoration, close, and Main invariants

**Files:**

- Create: `src/tui/app_manual_session/mod.rs`
- Create: `src/tui/app_manual_session/launch.rs`
- Create: `src/tui/app_manual_session/lifecycle.rs`
- Create: `src/tui/app_manual_session/restore.rs`
- Create: `src/tui/app_brain/tests/manual_session.rs`
- Create: `src/tui/app_brain/tests/manual_session/restoration.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app_brain/tests/mod.rs`
- Modify: `src/tui/app_brain/launch/session.rs`
- Modify: `src/tui/app_brain/lifecycle.rs`
- Modify: `src/tui/state/brain.rs`
- Modify: `src/tui/state/services.rs`
- Modify: `src/tui/app_state/construct.rs`
- Modify: `src/tui/runtime/builder.rs`
- Modify: `src/tui/runtime/mod.rs`
- Modify: `src/tui/runtime/shutdown.rs`
- Modify: `src/tui/runtime/tick.rs`
- Modify: `src/tui/handlers/input.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/app_skill_session/lifecycle.rs`
- Modify: `src/tui/app_brain/tests/fixtures.rs`
- Modify: `src/tui/app_brain/tests/fixtures/recording.rs`
- Modify: `src/tui/app_brain/tests/lifecycle.rs`
- Modify: `src/tui/app_brain/tests/launch.rs`

**Interfaces:**

- Produces: `App::start_manual_session`, `App::restore_manual_sessions`, `App::close_manual_session`, `App::close_active_user_session`, and `App::tick_manual_sessions`.
- Preserves: `App::open_or_focus_brain` as the Main selector/focus entry point and `AgentController` as the only frontend facade.
- Consumes: ordered `ManualSessionRecord` values loaded before initial panel launch and the Task 1 store operations through `AppServices`.

Use these exact App methods:

```rust
pub(crate) fn start_manual_session(&mut self, name: ManualSessionName);
pub(crate) fn restore_manual_sessions(&mut self);
pub(crate) fn close_manual_session(&mut self, id: SessionTabId);
pub(crate) fn close_active_user_session(&mut self);
pub(crate) fn tick_manual_sessions(&mut self);
pub(crate) fn release_manual_session_locks(&self) -> anyhow::Result<()>;
```

- [x] **Step 1: Write failing launch and close tests for all frontends**

Add a table-driven test using `AgentKind::ALL` and the existing real controller facade recording transport:

```rust
#[test]
fn a_named_manual_session_launches_fresh_in_a_selected_additional_tab_for_every_frontend() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let mut app = test_app(&temporary, &Cli::parse_from(["tasks"]), kind);
        let recording = TransportRecording::default();
        app.brain.replace_manual_transport(recording.transport());

        app.start_manual_session(ManualSessionName::parse("Atlas", &["Brain".to_owned()]).unwrap());

        assert_eq!(app.active_brain_tab_title(), Some("Atlas"));
        assert_eq!(recording.launch_specs().len(), 1);
        assert!(app.brain.is_manual_session_tab(app.effective_brain_tab()));
        assert_eq!(app.services.manual_sessions(&interactive_scope(&app)).unwrap().len(), 1);
    }
}
```

Add separate RED tests proving Main has no close path, closing an Additional manual session deletes only its mapping, `Ctrl+X` closes Additional Manual and Skill but not Main or Receiver, a closed neighbor does not repoint another stable ID, and synchronous spawn failure rolls back both new DB rows.

- [x] **Step 2: Run manual lifecycle tests and verify RED**

Run: `cargo test --release tui::app_brain::tests::manual_session`

Expected: compilation fails because the manual lifecycle APIs are missing.

- [x] **Step 3: Extract a common manual launch pipeline from Main**

Implement this internal request boundary in `app_manual_session/launch.rs`:

```rust
enum ManualLaunchTarget {
    Main,
    Additional {
        record: crate::manual_session::ManualSessionRecord,
        tab_id: Option<crate::tui::model::SessionTabId>,
    },
}

struct PreparedManualLaunch {
    target: ManualLaunchTarget,
    request: crate::agent::LaunchRequest,
    controller: crate::agent::AgentController,
    response_id: String,
    resumed_session_id: Option<String>,
}
```

Preparation must resolve capabilities and frontend response identity before taking a DB lock. Launch must use the exact manual-session ID as `BRAIN_INSTANCE_ID`, register or claim the exact native session before spawn, and roll back a fresh Additional mapping on synchronous spawn failure. Main without a saved mapping may scan ordinary recency candidates once, then attach the selected row as Main; restored manual sessions may only try their mapped native ID.

Keep `open_or_focus_brain(prompt)` as a thin Main operation. When Main is alive it selects Main, focuses the brain panel, and queues an optional prompt. When Main needs launching it delegates to the common pipeline.

- [x] **Step 4: Implement ordered startup restoration and exact shutdown release**

In `RuntimeBuilder::build_application`, create the interactive `SessionScope`, load `db.manual_sessions(&scope)`, and pass those records through `AppInit`. Use the stored Main ID when present; otherwise allocate a new one. Replace `launch_initial_agent_panel` with:

```rust
fn launch_initial_agent_sessions(app: &mut App) {
    crate::logging::log("workspace shared-server lease ready");
    app.restore_manual_sessions();
    app.focus_tasks();
}
```

`restore_manual_sessions` launches Main first and Additional rows in position order. Missing frontend evidence triggers `replace_manual_session` with a fresh native ID under the same title and position. A failed Additional launch leaves its durable row for the next restart and does not move focus or selection.

`BrainPanelState` always owns the Main manual-session identity even when its controller is temporarily unavailable. Its panel-visible decision therefore remains true, rendering the invariant-bearing Main tab and status instead of collapsing the brain panel.

Remove the single `instance: String` field from `TuiRuntime`. Add `App::release_manual_session_locks()` and call it at `ShutdownStage::ReleaseSessionLocks` after every controller has received shutdown. The store mapping must remain intact.

- [x] **Step 5: Implement manual close and recurring exit behavior**

`close_manual_session(SessionTabId)` must reject non-Manual metadata, shut down only that controller, close the exact durable mapping, remove the tab, and select Main only if the closed tab was active. `close_active_user_session()` dispatches by metadata: Additional Manual to `close_manual_session`, Skill to existing skill cleanup, Main and Receiver to no-op.

Rename `handle_skill_session_key` to `handle_session_tab_key` because it forwards input to Manual and Skill tabs as well as the receiver tab selected by existing switching logic. Do not mark receiver input as a manual turn. A normally exited Additional Manual closes and removes its mapping; a refused startup resume replaces the controller with a fresh session in the same tab. A normally exited Main relaunches in tab 0 and never hides the panel.

- [x] **Step 6: Run lifecycle tests and verify GREEN**

Run:

```sh
cargo test --release tui::app_brain::tests::manual_session
cargo test --release tui::app_brain::tests::lifecycle
cargo test --release tui::app_brain::tests::launch
cargo test --release tui::app_brain::tests::receiver_tab
cargo test --release tui::runtime::
```

Expected: manual persistence behavior passes, legacy Main resume tests pass with updated non-close semantics, and every receiver regression remains green.

---

### Task 4: Naming modal and global command-palette actions

**Files:**

- Create: `src/tui/app_brain/tests/manual_session/modal.rs`
- Create: `src/tui/tests/palette_sections/session_actions.rs`
- Modify: `src/tui/action/global.rs`
- Modify: `src/tui/modal_state.rs`
- Modify: `src/tui/modals.rs`
- Modify: `src/tui/overlay/mod.rs`
- Modify: `src/tui/event_loop/modal_route.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/handlers/overlay.rs`
- Modify: `src/tui/draw/mod.rs`
- Modify: `src/tui/draw_modals.rs`
- Modify: `src/tui/app_actions/commands.rs`
- Modify: `src/tui/palette/command.rs`
- Modify: `src/tui/palette/command/catalog.rs`
- Modify: `src/tui/palette/state.rs`
- Modify: `src/tui/tests/mod.rs`
- Modify: `src/tui/tests/palette_sections/skill_session_actions.rs`
- Modify: `src/tui/tests/palette_sections/global_action_identity.rs`
- Modify: `src/menu/model/mod.rs`
- Modify: `src/menu/model/tests.rs`
- Modify: `src/tui/state/shell.rs`
- Modify: `src/tui/search_view.rs`

**Interfaces:**

- Produces: `Overlay::ManualSessionName`, `ManualSessionNameState`, and `handle_manual_session_name_key`.
- Produces: `GlobalAction::StartManualSession`, `GlobalAction::ShowSessionTab(SessionTabId)`, and `GlobalAction::CloseSessionTab(SessionTabId)`.
- Removes: `GlobalAction::CloseBrain` and `GlobalAction::ShowSkillSession`.
- Consumes: `BrainPanelState::user_session_rows()` so both palette surfaces receive identical stable IDs and titles.

- [x] **Step 1: Write failing modal tests**

Add tests that route real key events through the overlay handler:

```rust
#[test]
fn manual_session_name_modal_keeps_blank_and_duplicate_input_visible() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = test_app(&temporary, &Cli::parse_from(["tasks"]), AgentKind::Claude);
    app.open_manual_session_name_modal();
    type_text(&mut app, " brain ");
    press_enter(&mut app);

    assert!(matches!(
        app.overlay.as_ref(),
        Some(Overlay::ManualSessionName(state)) if state.error() == Some("A session named Brain is already open")
    ));
    assert!(app.brain.user_session_rows().is_empty());
}

#[test]
fn escape_cancels_manual_session_naming_without_launching() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = test_app(&temporary, &Cli::parse_from(["tasks"]), AgentKind::Claude);
    app.open_manual_session_name_modal();
    press_escape(&mut app);
    assert!(app.overlay.is_none());
    assert!(app.brain.user_session_rows().is_empty());
}
```

Add a valid Enter test that asserts the trimmed title and fresh launch, plus backspace and Ctrl+U editing behavior.

- [x] **Step 2: Run modal tests and verify RED**

Run: `cargo test --release tui::app_brain::tests::manual_session::modal`

Expected: compilation fails because the overlay variant and handler do not exist.

- [x] **Step 3: Implement the captive single-line naming modal**

Add:

```rust
pub(crate) struct ManualSessionNameState {
    buffer: String,
    error: Option<String>,
}
```

The handler must swallow all modal input, support printable characters, Backspace, Ctrl+U, Esc, Ctrl+C, and Enter, and call `ManualSessionName::parse` with `Brain` plus all open manual titles. Validation failure stores the inline message without closing. Success closes the modal before `App::start_manual_session` to prevent the launch from racing another overlay.

Render a rounded, cyan-accent text modal titled `Start new brain session`, with prompt `What would you like to name this session?`, the current buffer, inline validation error, and `Enter start  Esc cancel` footer.

- [x] **Step 4: Write failing shared palette-row tests**

Construct one Main, one Additional Manual, one Daily triage Skill, and one Receiver row. Assert both palette builders contain these exact labels and stable actions:

```rust
let expected = [
    "Start new brain session",
    "Show main brain session",
    "Show Atlas session",
    "Close Atlas session",
    "Show Daily triage session",
    "Close Daily triage session",
];
```

Assert neither palette contains `Close Brain session`, `Close Receiver · SMS session`, or `Show Receiver · SMS session`. Assert no session action has a direct shortcut annotation. Assert logs and task-specific palettes retain their existing scopes.

- [x] **Step 5: Run palette tests and verify RED**

Run:

```sh
cargo test --release tui::tests::palette
cargo test --release menu::model::tests
```

Expected: assertions fail because Start and dynamic Close rows are missing and task/search catalogs differ.

- [x] **Step 6: Implement stable dynamic actions in both palette surfaces**

Use stable action identities:

```rust
pub(crate) enum GlobalAction {
    MessageBrain,
    StartManualSession,
    ShowMainBrainSession,
    ShowSessionTab(SessionTabId),
    CloseSessionTab(SessionTabId),
    ToggleReceiver,
    ToggleLayout,
    ShowTasks,
    ShowReceiverServerStatus,
    ShowReceiverServerLogs,
    ShowBrainLogs,
    OpenHabits,
    SyncBrainNow,
    ShowSyncStatus,
    OpenAgenda,
    ToggleDailyTriageAlert,
    RunSkillSession(SkillSessionKey),
}
```

Place `Start new brain session` immediately after `Message brain`; keep configured Run Skill rows in the same brain-session group; then append Main Show, per-tab Show, and per-tab Close rows. `Show main brain session` appears only when `user_session_rows` is nonempty. Both TaskPalette runtime context and search `Targets` receive the same cloned `Vec<SessionPaletteEntry>` from the App when the palette opens.

Dispatch `StartManualSession` to the naming modal, `ShowSessionTab(id)` to `select_brain_tab`, and `CloseSessionTab(id)` to the kind-gated close path. `MessageBrain` always selects Main. Remove every Close Brain row, shortcut, footer hint, and handler.

- [x] **Step 7: Run modal, palette, key-routing, drawing, and receiver tests and verify GREEN**

Run:

```sh
cargo test --release tui::app_brain::tests::manual_session::modal
cargo test --release tui::tests::palette
cargo test --release menu::model::tests
cargo test --release tui::tests::keymap
cargo test --release tui::draw::brain_panel::tests
cargo test --release tui::app_brain::tests::receiver_tab
```

Expected: the modal and both palettes agree, Main and Receiver cannot be closed, and existing switching remains green.

---

### Task 5: Lifecycle bridge rotation for every manual session

**Files:**

- Modify: `scripts/agent_session_start_hook.py`
- Modify: `tests/hook_integration.rs`
- Modify: `tests/hook_integration/lifecycle.rs`
- Modify: `tests/hook_integration/contracts.rs`
- Modify: `src/tui/app_brain/tests/lifecycle.rs`

**Interfaces:**

- Consumes: stable manual-session ID through `BRAIN_INSTANCE_ID` and the exact frontend/workspace/actor/channel scope.
- Produces: atomic synchronization of `brain_sessions.agent_session_id` and `manual_sessions.agent_session_id` after native session rotation.
- Preserves: fork rejection, receiver registration fencing, ambient no-op behavior, and silent hook failure.

- [x] **Step 1: Write a failing real-script rotation test**

Extend the existing hook integration fixture to insert a matching manual mapping, then run the real Python script for all frontends:

```rust
#[test]
fn native_rotation_updates_the_matching_manual_mapping_for_every_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let (_temporary, db) = fresh_db();
        register_manual_session(&db, agent_kind, "manual-id", "pending", "Atlas", 1);
        let payload = if agent_kind == "codex" {
            serde_json::json!({"thread_id":"rotated","source":"startup"})
        } else {
            serde_json::json!({"session_id":"rotated","source":"startup"})
        };

        let output = run_scoped_hook(&db, agent_kind, "pablo", "manual-id", &payload.to_string());

        assert!(output.status.success());
        assert_eq!(read_manual_native_id(&db, agent_kind, "manual-id"), Some("rotated".to_owned()));
    }
}
```

Add a receiver test proving an instance without a `manual_sessions` row still rotates only its receiver-owned `brain_sessions` row. Add wrong-scope and fork tests proving the mapping cannot be forged or displaced.

- [x] **Step 2: Run hook integration tests and verify RED**

Run: `cargo test --release --test hook_integration native_rotation_updates_the_matching_manual_mapping_for_every_frontend`

Expected: the `brain_sessions` row rotates but `manual_sessions.agent_session_id` remains `pending`.

- [x] **Step 3: Update the lifecycle transaction**

After the existing `brain_sessions` upsert succeeds and before prior lineage rows are unlocked, execute:

```sql
UPDATE manual_sessions
SET agent_session_id = ?
WHERE manual_session_id = ?
  AND agent_kind = ?
  AND workspace_id = ?
  AND actor_id = ?
  AND channel = ?;
```

Bind the reported native session ID, `BRAIN_INSTANCE_ID`, and immutable scope. A zero-row update is valid for receiver sessions and older databases. If the table is absent during downgrade compatibility, detect that condition and continue the existing `brain_sessions` transaction without raising from the hook.

Update the script docstring so `BRAIN_INSTANCE_ID` means one tracked controller lineage, not the whole shell.

- [x] **Step 4: Run the full hook, OpenCode plugin, stop-hook, and agent lifecycle suites and verify GREEN**

Run:

```sh
cargo test --release --test hook_integration
cargo test --release --test opencode_plugin
cargo test --release --test stop_hook_actor
cargo test --release tui::app_brain::tests::lifecycle
```

Expected: manual mappings rotate for all frontends and every receiver/ambient/fork fence remains green.

---

### Task 6: Documentation, release version, and complete verification

**Files:**

- Modify: `docs/glossary.md`
- Modify: `docs/features.md`
- Modify: `docs/architecture.md`
- Modify: `docs/data-model.md`
- Modify: `docs/integrations.md`
- Modify: `docs/keybindings.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: source comments found by the final terminology audit

**Interfaces:**

- Documents: Receiver sessions with Email/SMS subtypes, Skill sessions with Daily triage/user-specified subtypes, and Manual sessions with Main/additional subtypes.
- Documents: tab-0 invariant, naming modal, palette Show/Close rows, restart restoration, schema, hook rotation, and receiver exclusions.
- Produces: release version `0.87.0`.

- [x] **Step 1: Update the durable documentation contract**

Add the exact taxonomy to `docs/glossary.md`. Update feature and keybinding text so `Ctrl+X` closes only Additional Manual or Skill tabs and never Main or Receiver. Replace the old closed-panel and `Close brain` behavior with permanent Main-tab behavior. Describe the new store and startup data flow in architecture/data-model, the lifecycle bridge update in integrations, and the rationale for special Main plus typed additional tabs in decisions. Update testing.md with the new pure/store/app regression boundaries.

- [x] **Step 2: Run a terminology and product-name audit**

Run:

```sh
rg -n 'ephemeral tab|ephemeral_tab|EphemeralTab|Close brain|close brain|panel is closed|main session.*close|skill-session tab it closes only' src docs scripts tests
rg -n 'Receiver session|Email session|SMS session|Skill session|Daily triage session|Manual session|Main session' docs/glossary.md docs/features.md docs/architecture.md docs/data-model.md docs/integrations.md docs/keybindings.md docs/decisions.md
```

Every remaining `ephemeral` reference must refer specifically to Skill-session lifetime. No core source or documentation may gain extension-specific paths, filenames, URLs, or product names.

- [x] **Step 3: Bump the additive pre-1.0 release version**

Change the root package version in `Cargo.toml` and the `brain` package entry in `Cargo.lock`:

```toml
version = "0.87.0"
```

- [x] **Step 4: Format and run targeted verification**

Run:

```sh
cargo fmt --all -- --check
cargo test --release manual_session::
cargo test --release state::manual_session::
cargo test --release tui::app_brain::tests::manual_session
cargo test --release tui::app_brain::tests::receiver_tab
cargo test --release tui::tests::palette
cargo test --release --test hook_integration
```

Expected: formatting is clean and every targeted suite passes.

The final audit also identified missing production migration registration and
same-version repair for schema v14. Real-binary RED tests proved that downgrade
left v14 in place and that current reconciliation did not restore a deleted
managed index. The 0.87.0 startup migration now wires production up/down, repairs
missing mapping tables/indexes, and preserves native history and receiver v13
state before the existing downgrade chain. Focused GREEN tests cover upgrade,
idempotent downgrade, receiver-chain composition, and current reconciliation.
The full-suite read-only guard also exposed an unconditional v14 version stamp.
A dedicated real-binary RED/GREEN regression now proves healthy reconciliation
is byte-identical while missing mapping tables/indexes still repair. Existing
receiver downgrade fixtures and the semantic launch-owner guard were updated
to follow the new production seams without removing their prior assertions.

- [x] **Step 5: Run complete repository verification**

Run:

```sh
cargo test --release
cargo clippy --release --all-targets -- -D warnings
git diff --check
```

Expected: all tests pass, Clippy emits no warnings, and the diff contains no whitespace errors.

- [x] **Step 6: Review the final diff against the approved specification**

Check these literal outcomes in the implementation and tests:

```text
Main is Manual and tab 0.
Start new brain session asks for a unique name.
Additional Manual tabs restore in order.
Skill tabs do not persist.
Receiver tabs cannot be closed by the user.
Both palettes show the same manual and skill session actions.
Claude, Codex, and OpenCode rotate the exact manual mapping.
```

Verify no unrelated files from the dirty main checkout are present in this worktree diff.

- [x] **Step 7: Commit the verified implementation once**

Run:

```sh
git add Cargo.toml Cargo.lock src scripts tests docs
git commit -m "feat: add persistent manual brain sessions"
```

Expected: one implementation commit contains the feature, tests, docs, and `0.87.0` version bump. Branch publication and cleanup follow the repository-authorized finishing workflow only after this commit is verified.
