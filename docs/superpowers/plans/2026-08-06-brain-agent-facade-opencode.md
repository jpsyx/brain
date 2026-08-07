# Brain Agent Facade Completion and OpenCode Implementation Plan

> **For agentic workers:** REQUIRED SKILLS: Use `subagent-driven-development` to execute independent tasks, `test-driven-development` for every production change, and `rust-skills` for all Rust work. Use `systematic-debugging` before changing characterized behavior in response to a regression.

**Goal:** Make `AgentController` the only public interface for LLM operations, preserve complete Claude and Codex implementations behind that facade, and promote OpenCode from its fail-fast stub to a functional third frontend selected with `--open-code` or `-oc`.

**Architecture:** CLI, TUI, receiver, setup, doctor, and capability code issue frontend-neutral requests to `AgentController`. A private frontend registry constructs one `AgentFrontend` implementation for Claude, Codex, or OpenCode. The controller owns launch preparation, semantic input, session selection and registration, response identity, completion, and shutdown. Adapter-owned integration metadata drives hook or plugin installation, diagnostics, and capability preparation without frontend branches in shared callers.

**Tech Stack:** Rust 2024, clap, portable-pty, serde/serde_json, rusqlite, existing Python lifecycle hooks, a small OpenCode JavaScript plugin, and the installed Claude, Codex, and OpenCode CLIs. Do not add a Rust dependency unless the implementation proves the existing stack cannot satisfy a requirement and the dependency is justified in `docs/architecture.md`.

**Supersedes:** The OpenCode-stub portions and stub exit criteria in `docs/superpowers/plans/2026-08-02-brain-agent-controller-access.md`. That earlier plan remains the historical record of the facade foundation.

**Current crate version:** `0.36.1`. This is an additive pre-1.0 feature, so the completed implementation bumps the crate to `0.37.0` and updates `Cargo.lock` in the same verified change.

---

## Non-negotiable constraints

- Follow strict red/green/refactor TDD. Run each new test and observe the intended failure before writing the production code that satisfies it.
- Do not make live LLM requests in tests. Use recording adapters, fake CLI executables, fixture JSON, and PTY-level contract tests.
- Preserve existing Claude and Codex command, input, session, completion, receiver, and access-policy behavior unless a red test explicitly describes an approved correction.
- All LLM-specific behavior must be reachable through `AgentController`. Shared TUI, receiver, setup, doctor, and skills code must not call a concrete adapter directly.
- Keep the OpenCode implementation behind the same facade. Do not bypass the controller for launch, input, session tracking, completion, or delivery.
- Keep workspace-only enforcement claims honest. Prompt, MCP, skill, cwd, and configuration filtering are advisory unless a frontend-specific test proves exclusion of user and global sources.
- Preserve shared frontend authentication. Do not create alternate Claude homes, Codex homes, OpenCode homes, OS users, containers, or authentication profiles.
- Do not put secrets in process arguments, logs, generated docs, or response artifacts. Use inherited environment or owner-only temporary files.
- Keep state schema frontend-neutral. The existing `brain_sessions.agent_kind` and `agent_session_id` fields must support OpenCode without adding frontend-named columns.
- Keep modules cohesive and small. Split adapter implementations into owned submodules when command, input, session, and integration concerns would otherwise make one file oversized.
- Update all affected product documentation in the same implementation change.
- Run the public brain core verification suite before completion. After all required checks pass, follow the repository's authorized public brain core commit and push workflow.

---

## Current-state audit

The implementation begins from a partial facade, not from scratch.

### Already present

- `src/agent/frontend.rs` defines `AgentFrontend`, `LaunchRequest`, and `LaunchSpec`.
- `src/agent/controller/` provides semantic typing, submit, queue, new-session, completion, transcript, terminal, and shutdown methods.
- `src/agent/claude.rs` and `src/agent/codex.rs` are functional adapters.
- `src/agent/opencode.rs` is constructible but every operational method returns `UnsupportedFrontend`.
- `AgentKind::OpenCode`, `opencode_cmd`, `--open-code`, and alias normalization for `-oc` already exist.
- The state DB stores frontend-neutral `agent_kind` and `agent_session_id` values.
- The Python hook payload already reads `BRAIN_AGENT_KIND` and accepts `session_id` or `thread_id`.
- Brain skill fan-out already includes `~/.config/opencode/skills`.

### Facade leaks to remove

- `src/tui/app_brain/launch.rs` constructs a raw frontend and directly calls availability, resume validation, and response-ID methods before creating `AgentController`.
- `src/session/` retains frontend command and environment compatibility builders outside the facade.
- `src/command/server/receiver/hooks.rs` hard-codes Claude and Codex installation behavior.
- `src/command/server/receiver/setup/transaction.rs` hard-codes Claude and Codex rollback artifacts.
- `src/tasks/doctor/` models and renders Claude and Codex health as named fields.
- `src/skills/command.rs` hard-codes Claude and Codex capability columns and enforcement evidence.
- `src/command/tasks.rs` extracts only `--codex` and `-cx` from delegated task arguments.
- `src/workspace/bootstrap_policy.rs` ignores only Codex selectors during bootstrap command classification.
- `src/command/dispatch.rs` explicitly rejects OpenCode.
- TUI and integration tests use exhaustive matches that mark OpenCode unreachable.
- Product docs and `AGENTS.md` describe OpenCode as inert or fail-fast.

### OpenCode integration surface verified during planning

The current OpenCode CLI and official documentation provide the required seams:

- TUI launch supports `--session`, `--prompt`, and `--agent`.
- `/new` starts a new session.
- The TUI supports queued prompts while a session is busy.
- Project plugins can receive `session.created` and `session.idle` events.
- Plugins receive an SDK client that can read session messages.
- `OPENCODE_CONFIG_CONTENT` can provide highest-precedence inline configuration.
- Configuration supports named agents with trusted `prompt`, permissions, skills paths, and MCP definitions.

References:

- [OpenCode CLI](https://opencode.ai/docs/cli/)
- [OpenCode TUI](https://opencode.ai/docs/tui/)
- [OpenCode keybindings](https://opencode.ai/docs/keybinds/)
- [OpenCode plugins](https://opencode.ai/docs/plugins/)
- [OpenCode configuration](https://opencode.ai/docs/config/)
- [OpenCode MCP servers](https://opencode.ai/docs/mcp-servers/)

Do not encode an undocumented byte sequence or event payload from memory. Every OpenCode-specific command, key, event, and config field needs a focused contract test against a fixture or fake executable shaped like the supported OpenCode surface.

---

## Target component model

```text
CLI / TUI / Receiver / Setup / Doctor / Skills
                         |
                         v
                  AgentController
        launch, input, sessions, completion, shutdown
                         |
                         v
                 AgentFrontend trait
             /-----------+-----------\
            v            v            v
      ClaudeFrontend CodexFrontend OpenCodeFrontend
            |            |            |
            v            v            v
      integration   integration   integration
       metadata      metadata      metadata
```

Only these places may exhaustively match `AgentKind`:

1. CLI selection and user-facing labels.
2. The private registry or factory in `src/agent/`.
3. Adapter contract tests and fixtures.
4. Compatibility migration code that must recognize a legacy frontend-specific artifact.

Everything else iterates registry entries or calls the facade.

### Proposed semantic input model

Use a typed operation instead of exposing separate frontend-shaped methods:

```rust
pub enum AgentAction {
    SubmitNow,
    QueueAfterActiveTurn,
    StartNewSession,
}

pub trait AgentFrontend: Send {
    fn kind(&self) -> AgentKind;
    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError>;
    fn input_for(&self, action: AgentAction) -> Result<InputSequence, AgentError>;
    fn session_policy(&self) -> SessionPolicy;
    fn completion_strategy(&self) -> CompletionStrategy;
    fn integration(&self) -> FrontendIntegration;
}
```

The exact names may change during implementation, but the ownership boundary may not. Callers request meaning, adapters translate meaning into command arguments, terminal bytes, session rules, and integration artifacts.

### Proposed facade responsibilities

`AgentController` or a private child owned by it performs the complete launch transaction:

1. Ensure the configured frontend command is available.
2. Ask the adapter for resumable candidates or validate a stored candidate.
3. Select fresh versus resume using frontend-neutral state rules.
4. Allocate a real Brain-owned ID or a pending placeholder according to `SessionPolicy`.
5. Derive the stable response ID.
6. Register the pending or resumed session in `SessionStore`.
7. Prepare capability artifacts and environment.
8. Build one `LaunchSpec` through the adapter.
9. Spawn the transport.
10. Roll back registration and temporary artifacts if any step fails.

The TUI receives the resulting controller and public response identity. It does not see or call `AgentFrontend`.

### Frontend behavior matrix

| Concern | Claude | Codex | OpenCode |
| --- | --- | --- | --- |
| Fresh frontend ID | Brain passes `--session-id` | Frontend assigns it | Frontend assigns it |
| Initial DB identity | Real session ID | Pending placeholder | Pending placeholder |
| Resume command | Existing `--resume` behavior | Preserve current command support and current eligibility policy | `--session <id>` |
| Initial user prompt | Preserve current argument behavior | Preserve current argument behavior | `--prompt <text>` |
| Trusted Brain prompt | Existing Claude mechanism | Existing Codex mechanism | Named Brain agent in inline config |
| Submit | Enter | Enter | Enter |
| Queue while busy | Enter | Tab | Determine through PTY contract test, expected Enter |
| New session | `/new` then Enter | `/new` then Tab | `/new` then Enter |
| Completion | Hook | Hook | `session.idle` plugin event |
| Real-ID discovery | SessionStart hook | SessionStart hook | `session.created` plugin event |
| Final response | Hook payload or transcript fallback | Hook payload | SDK message lookup, then generic completion bridge |
| Receiver resume | Supported | Preserve current limitation | Supported after session validation |

---

## Task 1: Freeze the Claude and Codex facade contract

**Files:**

- Modify: `src/agent/adapter_tests.rs`
- Modify: `src/agent/controller/tests.rs`
- Modify: `tests/agent_characterization.rs`
- Modify: `tests/agent_access_adapter.rs`
- Modify: `src/tui/app_brain/tests/launch.rs`
- Modify: `src/tui/app_brain/tests/lifecycle.rs`

- [ ] **RED/GREEN characterization: fill gaps without changing production behavior**

Add table-driven tests for Claude and Codex covering:

- Configured command prefix parsing and default command fallback.
- Fresh and resumed launch commands.
- Workspace cwd from the first process instant.
- Trusted boundary prompt placement.
- User prompt quoting, including empty text, quotes, newlines, and leading hyphens.
- Environment identity: workspace UUID, actor, channel, frontend kind, pending or real session ID, and response ID.
- Typed text without submit.
- Submit-now sequence.
- Queue-after-turn sequence and delayed delivery.
- New-session sequence.
- Completion strategy.
- Transcript discovery and resume validation.
- Response-ID determinism.
- Receiver resume eligibility.
- Capability artifact cleanup after shutdown and failed launch.

- [ ] Add a single adapter-contract helper used by both Claude and Codex tests. The helper must assert semantics, while adapter-specific expected command arguments and input bytes remain explicit fixtures.

- [ ] Run every new characterization test before modifying production code. If an assertion does not match current behavior, inspect the implementation and adjust the test to the actual contract unless this plan explicitly changes that behavior.

- [ ] Establish the green baseline:

```sh
cargo test --release agent::
cargo test --release --test agent_characterization
cargo test --release --test agent_access_adapter
cargo test --release tui::app_brain::tests
```

**Exit criteria:** Claude and Codex behavior is fully described by tests that can be reused when the facade boundary changes.

---

## Task 2: Make launch and session orchestration facade-owned

**Files:**

- Modify: `src/agent/controller/mod.rs`
- Create: `src/agent/controller/launch.rs`
- Create: `src/agent/controller/session.rs`
- Modify: `src/agent/controller/tests.rs`
- Modify: `src/agent/controller/test_support.rs`
- Modify: `src/agent/frontend.rs`
- Modify: `src/agent/session.rs`
- Modify: `src/tui/app_brain/launch.rs`
- Modify: `src/tui/app_brain/receiver/state.rs`
- Modify or remove: `src/session/mod.rs`
- Modify or remove: `src/session/tests.rs`

- [ ] **RED: prove one facade call owns the launch transaction**

Add a recording frontend, transport, session store, and capability preparer. Test that a controller launch request produces this ordered behavior:

```text
availability -> candidate validation -> identity allocation -> state registration
-> capability preparation -> launch-spec construction -> transport spawn
```

Add failure tests at each boundary. A failure must:

- Release any claimed resumable session.
- Remove any pending session registration created by this launch.
- Clear the public response identity.
- Clean temporary capability artifacts.
- Avoid spawning the transport when preparation failed.
- Shut down a spawned transport if a post-spawn registration step ever becomes necessary and fails.

- [ ] **RED: prove TUI launch does not need a raw adapter**

Refactor the test fixture API first so `app_brain` tests receive an `AgentControllerFactory` or equivalent facade constructor. The test should fail while `launch.rs` still calls `configured_frontend`, `ensure_available`, `resume_candidate_exists`, or `response_id` directly.

- [ ] **GREEN: introduce a facade-owned launch result**

Return a value such as:

```rust
pub struct PreparedAgentLaunch {
    pub controller: AgentController,
    pub response_id: String,
    pub resumed: bool,
}
```

Keep frontend session IDs private unless a frontend-neutral caller genuinely needs one. Prefer controller methods for later state queries.

- [ ] Move candidate selection, validation, placeholder allocation, response identity, and registration from `src/tui/app_brain/launch.rs` into `src/agent/controller/session.rs`.

- [ ] Move command/environment compatibility code from `src/session/` behind the registry and adapters. Reduce `src/session/mod.rs` to re-exports only if public paths still need compatibility; otherwise remove it and update imports.

- [ ] Replace direct `can_resume_response_session` calls in receiver state with a facade query that does not expose adapter policy.

- [ ] Run the focused tests and keep the Task 1 characterization suite green.

**Exit criteria:** No TUI or receiver module constructs or invokes an `AgentFrontend`. Session planning and response identity are controller-owned.

---

## Task 3: Replace input-shaped adapter methods with semantic actions

**Files:**

- Modify: `src/agent/frontend.rs`
- Modify: `src/agent/input.rs`
- Modify: `src/agent/controller/mod.rs`
- Modify: `src/agent/claude.rs`
- Modify: `src/agent/codex.rs`
- Modify: `src/agent/opencode.rs`
- Modify: `src/agent/adapter_tests.rs`
- Modify: `src/agent/controller/tests.rs`

- [ ] **RED: define semantic action tests**

Use a recording adapter to prove:

```rust
controller.type_text("draft")?;
controller.submit_now()?;
controller.queue_after_active_turn("follow-up")?;
controller.start_new_session()?;
```

produces semantic frontend requests in the expected order, while only the adapter test knows the final bytes.

- [ ] Test invalid sequencing:

- Empty queued text does not create a delayed submission.
- A second queued request has a documented replace-or-append policy.
- Shutdown clears delayed input.
- A dead transport rejects semantic input without mutating pending state.
- Starting a new session clears any delayed queue operation.

- [ ] **GREEN: add `AgentAction` and `input_for`**

Replace `submit_input`, `queue_input`, and `new_session_input` with one typed adapter translation. Keep literal text delivery in the controller or transport because text itself is frontend-neutral.

- [ ] Preserve Claude and Codex byte sequences exactly through the characterization tests.

- [ ] Keep delayed terminal timing in the controller, expressed as a pending semantic action rather than a pending frontend key.

**Exit criteria:** Shared code contains no knowledge that Claude queues with Enter or Codex queues with Tab.

---

## Task 4: Add a complete frontend registry

**Files:**

- Create: `src/agent/registry.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/agent/session.rs`
- Modify: `src/agent/frontend.rs`
- Modify: `src/env/schema.rs`
- Modify: `src/env/vars/tests.rs`
- Modify: `src/skills/command.rs`
- Modify: `src/tasks/doctor/mod.rs`

- [ ] **RED: require every frontend to provide a complete registration**

Add `AgentKind::ALL` and a table-driven test that each kind provides:

- Stable machine-readable name.
- User-facing label.
- Environment key for the configured command.
- Default command.
- Adapter constructor.
- Session policy.
- Completion strategy.
- Integration artifact description.
- Capability enforcement evidence.
- Health-check rows.

- [ ] Add a test that the registry contains exactly one entry per `AgentKind::ALL` value and no duplicate names or flags.

- [ ] **GREEN: centralize construction and metadata**

Make the registry or one adjacent factory the only exhaustive production match for adapter construction and configured command resolution.

- [ ] Refactor shared capability and doctor code to consume registry entries instead of named Claude and Codex fields. It is acceptable for the adapter-owned integration descriptor to contain frontend-specific paths and schemas.

- [ ] Add an audit command to the task notes and run it after every later task:

```sh
rg -n 'AgentKind::(Claude|Codex|OpenCode)|ClaudeFrontend|CodexFrontend|OpenCodeFrontend' src
```

Every result outside `src/agent/`, CLI labels, compatibility migration, and tests needs explicit justification or removal.

**Exit criteria:** Adding a future frontend requires one registry entry and one adapter, not edits across doctor, setup, skills, and TUI.

---

## Task 5: Complete OpenCode CLI selection and delegated argument handling

**Files:**

- Modify: `src/cli/global.rs`
- Modify: `src/command/tasks.rs`
- Modify: `src/command/dispatch.rs`
- Modify: `src/workspace/bootstrap_policy.rs`
- Modify: `src/env/schema.rs`
- Modify: `src/main.rs`
- Modify: `tests/opencode_smoke.rs`
- Add or modify focused CLI tests under `tests/`

- [ ] **RED: cover every accepted selector position**

Add parse and dispatch tests for:

```text
brain --open-code
brain -oc
brain --open-code tasks
brain -oc tasks today --no-tui
brain tasks --open-code
brain tasks today -oc
```

The last two forms exercise delegated task parsing rather than Clap's initial global-option position.

- [ ] Test conflict handling for every mixed spelling of Codex and OpenCode selectors. The plain-text error remains:

```text
🔴 Choose one agent frontend: --codex or --open-code.
```

- [ ] Test that bootstrap policy classification ignores `--open-code` and `-oc` in the same way as Codex selectors.

- [ ] Replace the smoke assertion that OpenCode exits before bootstrap with an assertion that dispatch proceeds to the normal TUI startup boundary.

- [ ] **GREEN: generalize delegated selection**

Replace `take_codex_flag` with a parser returning zero or one `AgentKind`, while preserving unrelated task arguments. Duplicate same-kind selectors may normalize to one selection; conflicting kinds return the typed conflict error.

- [ ] Remove `validate_agent_kind`'s OpenCode rejection. Remove the function entirely if no other validation remains.

- [ ] Update CLI comments, help text, and `opencode_cmd` schema text to describe a functional frontend.

**Exit criteria:** `--open-code` and `-oc` work before or after delegated task arguments, and no startup gate rejects OpenCode.

---

## Task 6: Implement OpenCode launch, input, and session policy

**Files:**

- Replace: `src/agent/opencode.rs`
- Optionally create: `src/agent/opencode/mod.rs`
- Optionally create: `src/agent/opencode/command.rs`
- Optionally create: `src/agent/opencode/input.rs`
- Optionally create: `src/agent/opencode/session.rs`
- Modify: `src/agent/adapter_tests.rs`
- Replace or split: `tests/opencode_smoke.rs`
- Add fixture helpers under `tests/support/` if an existing focused support module is not appropriate

- [ ] **RED: test fresh command construction**

The launch specification must:

- Use `opencode` when `opencode_cmd` is blank.
- Preserve a configured command prefix and configured options.
- Set the selected workspace root as cwd.
- Select the generated Brain agent with `--agent brain`.
- Pass initial user text through `--prompt` as a separate argument.
- Avoid passing `--session` for a fresh launch.
- Populate all frontend-neutral `BRAIN_*` identity variables.
- Keep secrets out of the command.

- [ ] **RED: test resumed command construction**

A valid resume must add `--session <frontend-session-id>` without converting that ID into shell text. Test spaces, quotes, and leading hyphens as rejected session IDs or safe distinct arguments according to the existing `AgentSession` validation contract.

- [ ] **RED: test semantic input**

Initially encode the researched expectation:

```text
SubmitNow             -> Enter
QueueAfterActiveTurn  -> Enter while busy
StartNewSession       -> /new then Enter
```

Before making the queue assertion permanent, run a PTY contract fixture against the supported OpenCode TUI behavior. The test must observe queued-prompt state or a fake state-machine equivalent, not merely assert a guessed byte.

- [ ] **RED: test session identity**

Prove that:

- Fresh launch registers a pending placeholder because OpenCode assigns the real ID.
- Stable response ID derivation from the placeholder is deterministic and UUID-shaped.
- Resume validation is read-only.
- A missing or malformed OpenCode session is rejected before transport spawn.
- Valid resumed sessions retain the initiating actor and response identity.

- [ ] **GREEN: implement the adapter**

Store the configured command, implement launch specs, map semantic input, expose hook completion, and implement the OpenCode session policy.

- [ ] Use a documented read-only command such as `opencode session list --format json`, or a documented storage API, to validate resume candidates. Keep subprocess and parsing details inside the adapter. Do not let TUI code inspect OpenCode state.

- [ ] Add a friendly preflight error when the configured OpenCode executable is missing or lacks required flags. If a minimum supported version is necessary, define and test the comparison as a pure function and document it in `docs/integrations.md`.

**Exit criteria:** The OpenCode adapter can build fresh and resumed launches and translate all semantic input without lifecycle events yet.

---

## Task 7: Generalize the lifecycle bridge scripts

**Files:**

- Create: `scripts/agent_session_start_hook.py`
- Create: `scripts/agent_turn_complete_hook.py`
- Retain temporarily or remove after migration: `scripts/claude_session_start_hook.py`
- Retain temporarily or remove after migration: `scripts/claude_stop_hook.py`
- Modify: `tests/hook_integration.rs`
- Modify: `tests/stop_hook_actor.rs`
- Modify: `tests/agent_characterization.rs`
- Modify: `src/command/server/receiver/hooks.rs`

- [ ] **RED: express a frontend-neutral event contract**

The start bridge accepts normalized JSON:

```json
{
  "session_id": "frontend-session-id",
  "source": "startup"
}
```

The completion bridge accepts:

```json
{
  "session_id": "frontend-session-id",
  "last_assistant_message": "final response"
}
```

Both use `BRAIN_AGENT_KIND`, workspace, actor, channel, launch ID, pending session ID, response ID, and DB path from trusted environment.

- [ ] Add tests running the scripts for `claude`, `codex`, and `opencode` payloads. Cover `session_id`, the legacy Codex `thread_id`, duplicate delivery, stale launch IDs, wrong frontend, wrong workspace, and malformed JSON.

- [ ] Prove the start bridge atomically rotates only the exact pending lineage and does not claim another panel's or child agent's session.

- [ ] Prove the completion bridge writes at most one attributed response artifact and never falls back to a Claude transcript for Codex or OpenCode.

- [ ] **GREEN: rename the generic implementation**

Move frontend-neutral logic into the new scripts. Preserve Claude transcript fallback as an explicitly Claude-only branch selected by `BRAIN_AGENT_KIND`.

- [ ] Update hook replacement logic to recognize both old Claude-named paths and new generic paths. Existing installations must converge without duplicate hooks.

- [ ] Decide whether the old scripts become tiny compatibility launchers or are removed after installer migration. In either case, tests must prove stale registrations are replaced safely.

**Exit criteria:** One normalized start and completion contract works for all three frontends, with legacy Claude/Codex payload compatibility isolated at the boundary.

---

## Task 8: Install an OpenCode lifecycle plugin

**Files:**

- Create: `scripts/opencode_brain_plugin.js`
- Optionally create a focused template/renderer module under `src/agent/opencode/`
- Modify: `src/agent/opencode.rs` or `src/agent/opencode/integration.rs`
- Modify: `src/command/server/receiver/hooks.rs`
- Modify: `src/command/server/receiver/hooks/tests_parts/`
- Add: OpenCode plugin fixture tests under `tests/`

- [ ] **RED: test `session.created` translation**

Use a JavaScript fixture harness or a small process-level harness with a fake SDK client. A root `session.created` event must invoke the generic start bridge with the real session ID. A child session with `parentID` must be ignored.

- [ ] **RED: test `session.idle` translation**

The plugin must:

1. Confirm the event belongs to the tracked root session.
2. Read messages through the provided OpenCode client.
3. Select the newest completed assistant message for that session.
4. Join or select text parts deterministically.
5. Invoke the generic completion bridge with normalized JSON on stdin.

Test no messages, thinking-only messages, tool-only messages, multiple text parts, repeated idle events, and SDK errors. Errors should be logged without emitting a false completion.

- [ ] **RED: test safe process invocation**

The plugin must not interpolate response text or session IDs into a shell command. Pass the script path as an argument and JSON through stdin. Inherit only the required `BRAIN_*` values plus normal runtime necessities.

- [ ] **GREEN: install a workspace-local plugin**

Install the plugin under the selected workspace's `.opencode/plugins/` directory or the documented singular compatibility directory if required by the supported OpenCode version. Prefer one canonical target and document any compatibility handling.

- [ ] Make installation idempotent and owner-safe. Preserve unrelated user plugins and settings.

- [ ] Keep the plugin thin. Session authorization, DB mutation, deduplication, and artifact writing remain in the generic lifecycle bridge, not JavaScript.

**Exit criteria:** OpenCode can rotate a pending session and deliver the final assistant response through the same state and response pipeline as Claude and Codex.

---

## Task 9: Add OpenCode capability preparation

**Files:**

- Modify: `src/access/mod.rs`
- Modify or split: `src/access/mcp/`
- Create as needed: `src/access/opencode.rs`
- Modify: `src/workspace/paths.rs`
- Modify: `src/agent/opencode.rs` or `src/agent/opencode/integration.rs`
- Modify: `tests/workspace_capabilities.rs` and its focused parts
- Modify: `tests/agent_access_adapter.rs`

- [ ] **RED: test trusted prompt separation**

Generate an OpenCode configuration containing a named `brain` agent whose `prompt` contains the workspace and actor boundary. The user's initial message must remain only in the launch request's `--prompt` value.

Test that untrusted user text resembling JSON, OpenCode config, or a system prompt cannot alter the generated agent definition.

- [ ] **RED: test config merging**

If the parent environment already has valid `OPENCODE_CONFIG_CONTENT`, preserve unrelated user keys while Brain takes ownership of only its reserved agent name, selected MCP entries, and required plugin/config fields. Reject malformed inherited JSON with a clear error rather than silently discarding it.

- [ ] **RED: test MCP translation**

For each selected logical MCP:

- Translate local command, arguments, and credential environment without exposing values in output.
- Translate remote URL and required headers through the documented schema.
- Mark missing machine credentials unavailable.
- Avoid enabling an MCP excluded by the workspace capability plan.

- [ ] **RED: test skill selection**

Point OpenCode at the actor-specific rendered capability directory. Apply skill permission rules that deny unselected skills and allow selected skills where the documented schema supports this.

- [ ] **RED: test honest evidence**

Do not report strict exclusion merely because the generated config lists selected entries. Strict status requires a test showing that global/user MCP and skill sources are not still available. Otherwise report `AdvisoryOnly`.

- [ ] **GREEN: build inline OpenCode configuration**

Use serialized JSON assigned to `OPENCODE_CONFIG_CONTENT`, not shell quoting. Launch with `--agent brain`. Keep any credential-bearing wrapper or temporary artifact in the UUID-scoped capability cache with owner-only permissions.

- [ ] Add OpenCode cleanup to the same facade-owned launch rollback and shutdown paths as Claude and Codex.

**Exit criteria:** OpenCode receives Brain's trusted advisory prompt and selected capabilities through adapter-owned configuration, with accurate enforcement reporting and no leaked secrets.

---

## Task 10: Make hook installation, rollback, doctor, and status registry-driven

**Files:**

- Modify or split: `src/command/server/receiver/hooks.rs`
- Modify: `src/command/server/receiver/setup/transaction.rs`
- Modify: `src/command/server/receiver/setup/transaction/tests.rs`
- Modify: `src/tasks/doctor/mod.rs`
- Modify: `src/tasks/doctor/tests.rs`
- Modify: `src/skills/command.rs`
- Modify: `tests/doctor_integration.rs`
- Modify: `tests/status_read_only_parts/part_02.rs`

- [ ] **RED: define generic installation artifacts**

Model installation and health as a collection of rows rather than named fields:

```rust
pub struct FrontendHealth {
    pub kind: AgentKind,
    pub checks: Vec<HealthCheck>,
}
```

Test that all `AgentKind::ALL` values contribute required checks and that healthy status requires every required integration for every functional frontend.

- [ ] **RED: extend transactional rollback**

Snapshot and restore:

- Generic lifecycle scripts.
- Claude settings.
- Codex hooks settings.
- OpenCode Brain plugin.
- Any generated OpenCode integration config that setup owns.

Inject failure after every write step. Each test must prove byte-for-byte restoration of pre-existing files and removal of newly created owned files and empty owned directories.

- [ ] **RED: generalize capability rendering**

Replace fixed `Claude=` and `Codex=` output with deterministic registry-driven output that includes OpenCode. A row-oriented layout is preferred if three columns make the terminal line too wide.

- [ ] **GREEN: move artifact knowledge into adapter integration descriptors**

Shared setup code owns locking, atomic writes, rollback, and error rendering. Adapter integration descriptors own paths, config schemas, matcher names, and expected health checks.

- [ ] Rename fields such as `claude_hook_installed` and `codex_hook_installed`. Keep legacy path matching only in compatibility code.

- [ ] Ensure read-only status and doctor commands do not install, rewrite, or repair integrations.

**Exit criteria:** Setup, rollback, doctor, and capability status support all registered frontends without exhaustive frontend branches.

---

## Task 11: Complete TUI, receiver, and lifecycle acceptance coverage

**Files:**

- Replace: `tests/opencode_smoke.rs`
- Modify: `src/tui/app_brain/tests/fixtures.rs`
- Modify: `src/tui/app_brain/tests/launch.rs`
- Modify: `src/tui/app_brain/tests/lifecycle.rs`
- Modify: `src/tui/app_brain/tests/receiver.rs`
- Modify: `src/tui/app_brain/tests/receiver_sync.rs`
- Modify: `src/tui/app_brain/tests/triage.rs`
- Modify: `tests/agent_characterization.rs`
- Modify: `tests/hook_integration.rs`
- Modify: `tests/stop_hook_actor.rs`

- [ ] **RED: add a fake OpenCode executable**

Create a focused fixture that can:

- Record argv, cwd, and selected environment without recording secret values.
- Behave as a long-lived PTY process.
- Record terminal input bytes.
- Return fixture output for version and session-list probes.
- Trigger normalized session-created and session-idle fixture events.
- Exit on request so shutdown behavior is testable.

- [ ] Add end-to-end tests for:

1. `--open-code` fresh panel launch.
2. `-oc` fresh panel launch.
3. Initial prompt delivery.
4. Typing without submission.
5. Immediate submission.
6. Queueing while the active turn is busy.
7. Starting a new chat.
8. Pending placeholder rotation to the real OpenCode session ID.
9. Completion artifact creation with actor and channel attribution.
10. Receiver delivery from an authenticated inbound job.
11. Valid session resume after restart.
12. Missing session fallback to a fresh launch.
13. Triage's ephemeral agent launch.
14. Agent exit closing only the brain panel.
15. TUI close shutting down the OpenCode transport once.
16. Missing executable and unsupported-version diagnostics.

- [ ] Run the same frontend-neutral controller contract suite for Claude, Codex, and OpenCode. Keep only command, byte, session-policy, and integration fixtures adapter-specific.

- [ ] Remove every `AgentKind::OpenCode => unreachable!(...)` and stub expectation from tests.

- [ ] **GREEN:** make the smallest production changes required by each failing acceptance test. Do not broaden implementation while a narrower red test is pending.

**Exit criteria:** OpenCode satisfies the same facade-level lifecycle, prompt, completion, and delivery contract as the other functional frontends, with explicitly documented frontend differences.

---

## Task 12: Update documentation and remove the stub contract

**Files:**

- Modify: `AGENTS.md`
- Modify: `docs/README.md`
- Modify: `docs/glossary.md`
- Modify: `docs/architecture.md`
- Modify: `docs/features.md`
- Modify: `docs/keybindings.md`
- Modify: `docs/integrations.md`
- Modify: `docs/config.md`
- Modify: `docs/data-model.md`
- Modify: `docs/testing.md`
- Modify: `docs/decisions.md`
- Modify historical specs only if they incorrectly claim to describe current behavior rather than their original design scope

- [ ] Update `AGENTS.md` so the project summary and frontend parity rule describe OpenCode as functional and require all three adapters for future LLM capability work.

- [ ] Document the final module tree and facade data flow in `docs/architecture.md`.

- [ ] Document `--open-code` / `-oc`, launch behavior, queueing, new-session behavior, resume, and errors in `docs/features.md` and any current mention in `docs/keybindings.md`.

- [ ] Document command construction, inline configuration, plugin installation, event translation, generic lifecycle scripts, session-ID rotation, completion delivery, and supported-version policy in `docs/integrations.md`.

- [ ] Update `opencode_cmd` from reserved stub to functional machine-local command in `docs/config.md` and `docs/data-model.md`.

- [ ] Document frontend-neutral session states and pending-to-real ID rotation for Codex and OpenCode in `docs/data-model.md`.

- [ ] Record the decisions to:

- Keep one public facade with private adapters.
- Use semantic input actions.
- Use an OpenCode project plugin for lifecycle events.
- Use a pending placeholder for frontend-assigned fresh IDs.
- Use inline OpenCode config and a named Brain agent.
- Report capability filtering honestly as strict, advisory, or unavailable.
- Preserve shared frontend authentication.

- [ ] Document fixture-based OpenCode testing and the deliberate absence of live provider calls in `docs/testing.md`.

- [ ] Remove current-behavior statements containing `stub`, `inert`, `fail-fast`, or `not supported` for OpenCode. Preserve those words in historical plans when they describe what an earlier phase intentionally did.

**Exit criteria:** Current docs describe what the completed product does and why; historical plans remain truthful historical records.

---

## Task 13: Version, audit, and full verification

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify any files identified by the final branch and stub audit

- [ ] Bump the crate version from `0.36.1` to `0.37.0` and regenerate `Cargo.lock` through Cargo rather than editing lockfile package data manually.

- [ ] Format and run focused suites first:

```sh
cargo fmt --check
cargo test --release agent::
cargo test --release --test agent_characterization
cargo test --release --test agent_access_adapter
cargo test --release --test opencode_smoke
cargo test --release --test hook_integration
cargo test --release --test stop_hook_actor
cargo test --release tui::app_brain::tests
cargo test --release tasks::doctor::
```

If `tests/opencode_smoke.rs` is renamed during the implementation, substitute its final integration-test name.

- [ ] Run required public brain core verification:

```sh
cargo test --release bundled_skills_carry_no_personal_data
cargo test --release
cargo clippy --release --all-targets -- -D warnings
```

- [ ] Build release and run headless smoke paths that cannot open a TUI:

```sh
cargo build --release
./target/release/brain --help
./target/release/brain env get opencode_cmd
./target/release/brain workspace list
./target/release/brain server status
./target/release/brain tasks today --no-tui
```

- [ ] Run final source audits:

```sh
rg -n 'AgentKind::(Claude|Codex|OpenCode)|ClaudeFrontend|CodexFrontend|OpenCodeFrontend' src
rg -n 'OpenCode.*(stub|inert|fail-fast|not supported)|opencode.*(stub|inert)' AGENTS.md docs src tests
rg -n 'claude_session_start_hook|claude_stop_hook' src scripts tests docs
rg -n 'take_codex_flag|validate_agent_kind' src tests
```

Classify every surviving match. Frontend-specific operational matches are allowed only inside adapters, registry construction, compatibility migration, CLI labels, and adapter tests.

- [ ] Inspect production and test file sizes and responsibilities. Split any oversized or multi-responsibility module along owned seams before completion.

- [ ] Review every generated or stored text change for accidental secrets, personal paths, and unnecessary em dashes.

- [ ] After the complete suite passes, follow the public brain core workflow to commit and push the verified change to `main`. Do not mix unrelated working-tree changes into that commit.

---

## Final acceptance criteria

### Facade

- `AgentController` is the sole public entry point for launching, typing, submitting, queueing, starting new chats, selecting sessions, tracking IDs, receiving completion, and shutting down an LLM frontend.
- TUI, receiver, setup, doctor, and skills code do not call concrete adapters.
- Shared code issues semantic input operations and contains no frontend key sequences.
- Session registration and launch rollback are atomic from the caller's perspective.

### Frontends

- Claude remains fully functional with characterized behavior unchanged.
- Codex remains fully functional with characterized behavior unchanged.
- OpenCode supports fresh launch, initial prompt, typed input, immediate submit, busy-turn queueing, new chat, completion, stable response identity, session resume, receiver delivery, triage, and clean shutdown.
- `--open-code` and `-oc` work in all supported global and delegated CLI positions.

### Integration

- OpenCode lifecycle events rotate pending IDs and deliver responses through the same frontend-neutral state pipeline as Claude and Codex.
- Hook and plugin installation is idempotent, transactional, and preserves unrelated user configuration.
- Doctor and status report all registered frontends without hard-coded Claude/Codex structures.
- Capability preparation includes OpenCode, keeps secrets out of argv and logs, cleans temporary artifacts, and does not overstate enforcement.

### Quality

- Every production behavior was preceded by a test observed failing for the intended reason.
- No live LLM calls are required by the test suite.
- Current documentation contains no OpenCode stub contract.
- `cargo fmt --check`, full release tests, the personal-data guard, and strict Clippy all pass.
- The crate version is `0.37.0`.
