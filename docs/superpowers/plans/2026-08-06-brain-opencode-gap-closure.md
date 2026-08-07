# Brain OpenCode Completion Implementation Plan

> **Execution:** Use `subagent-driven-development` to execute this plan one task at a time. Every production behavior follows the repository's red, green, refactor loop. Run the spec-compliance review before the code-quality review for each task.

**Goal:** Make OpenCode a fully supported Brain frontend with the same facade-level launch, input, session, completion, receiver, triage, and shutdown contract as Claude and Codex, while preserving user OpenCode configuration and reporting unsupported integration states honestly.

**Baseline:** Commit `c385c10`, Brain `0.37.0`, and locally installed OpenCode `1.18.14`. The current code launches OpenCode and installs a plugin, but it has not passed a real OpenCode lifecycle acceptance test. This plan closes the remaining gaps discovered after that implementation.

**Architecture:** Keep `AgentController` as the only application-facing LLM facade. Concrete adapters own command syntax, semantic input translation, frontend session discovery, trusted configuration, lifecycle integration descriptors, and health probes. Shared code owns state transactions, PTY transport, rollback, rendering, and receiver orchestration. OpenCode lifecycle events flow through a thin project plugin into frontend-neutral Brain bridge scripts.

**Current OpenCode facts verified on 2026-08-06:**

- OpenCode `1.18.14` supports TUI flags `--agent`, `--prompt`, and `--session`, plus `opencode session list --format json` and `opencode export <sessionID>`.
- Project plugins are loaded from `.opencode/plugins/`.
- `OPENCODE_CONFIG_CONTENT` is the runtime inline-config layer; OpenCode merges it after project config.
- `session.created` contains `properties.info`, including `id`, `directory`, and optional `parentID`.
- `session.idle` contains only `properties.sessionID`; it does not contain parent metadata.
- OpenCode `1.18.14` constructs plugin clients from the v1 JavaScript SDK; session lookup uses `{ path: { id }, query: { directory } }`. The separately exported v2 SDK uses flat `{ sessionID, directory }` parameters but is not the plugin client supplied by this release.
- `/new` is a documented TUI command for creating a new session.

Primary references:

- <https://dev.opencode.ai/docs/cli/>
- <https://dev.opencode.ai/docs/config>
- <https://opencode.ai/docs/plugins/>
- <https://opencode.ai/docs/tui/>
- <https://github.com/anomalyco/opencode/blob/v1.18.14/packages/plugin/src/index.ts>
- <https://github.com/anomalyco/opencode/blob/v1.18.14/packages/sdk/js/src/v2/gen/sdk.gen.ts>

---

## Confirmed gap inventory

| Priority | Gap | Current evidence | Required outcome |
| --- | --- | --- | --- |
| P0 | Plugin message lookup must stay aligned with the plugin client SDK | OpenCode `1.18.14` supplies the v1 client, whose generated API requires `{path:{id}, query:{directory}}`; v2 documentation uses a different flat shape | Completion lookup works against the client OpenCode actually supplies to plugins |
| P0 | Child-session idle events can be treated as root completion | `session.idle` has no `info`; current `isRootSession` returns true when `info` is absent | Resolve or remember session metadata and ignore every child session |
| P0 | Plugin errors are silent | Hook subprocess stderr is discarded and SDK failures are uncaught | Log lifecycle failures without publishing false completion |
| P1 | OpenCode availability check is a no-op | `AgentFrontend::ensure_available` defaults to `Ok(())` and OpenCode does not override it | Missing binary, incompatible CLI, and malformed probe output fail before PTY spawn |
| P1 | Resume validation always succeeds | `OpenCodeFrontend::resume_candidate_exists` returns `true` for every nonblank Brain row | Read-only discovery confirms the exact root session in the selected workspace |
| P1 | Completed receiver sessions claim resumability without proof | `can_resume_response_session` returns `true` unconditionally | Resume only when OpenCode still owns that session in the same workspace |
| P1 | Existing inline OpenCode config is discarded | Launch construction replaces `OPENCODE_CONFIG_CONTENT` | Preserve unrelated valid keys; Brain owns only its reserved agent and capability entries |
| P1 | OpenCode capability preparation is prompt-only | Adapter reports advisory evidence but does not translate selected MCPs or selected skills | Generate truthful OpenCode capability config and never overstate enforcement |
| P1 | Lifecycle scripts are Claude-named | OpenCode plugin executes scripts under `.claude/brain-hooks/claude_*` | Generic bridge names and contracts, with safe migration of old registrations |
| P1 | Queue semantics are guessed | Controller sends text, waits two event-loop ticks, then sends Enter | Define and test immediate submit versus busy-turn queue behavior per frontend |
| P1 | Delegated flags do not share conflict validation | `take_agent_flag` keeps the last delegated selector | One parser rejects every Codex/OpenCode conflict in every supported position |
| P2 | Facade registration still leaks concrete frontends | TUI factory, skills status, hook setup, and doctor contain concrete or fixed frontend knowledge | One registry supplies adapters, integration artifacts, health, and capability evidence |
| P2 | `transcript()` is a public facade method with no production caller | Only adapter/controller tests call it | Replace it with the session-validation operation callers actually need, or make it adapter-private |
| P2 | OpenCode parity coverage is incomplete | Several TUI lifecycle, receiver, triage, and characterization loops still list only Claude and Codex | Run the shared contract suite for every functional frontend |
| P2 | No real-process compatibility fixture exists | `tests/opencode_smoke.rs` checks generated strings and a recording transport only | A fake executable and plugin harness exercise argv, env, PTY bytes, events, and lifecycle bridges |
| P2 | Current docs contradict the implementation | Current docs still contain `stub`, `inert`, and unsupported claims | Current-behavior docs describe the final support contract consistently |

## Non-goals

- Do not call a paid or remote LLM in the automated test suite.
- Do not modify OpenCode's global authentication or provider credentials.
- Do not overwrite unrelated project plugins, agents, commands, skills, or configuration.
- Do not claim that prompt, MCP, or skill filtering is a security sandbox.
- Do not replace Brain's PTY panel with an OpenCode server client in this change.
- Do not add a fourth frontend while this parity work is in progress.

---

## Task 1: Characterize the remaining facade contract

**Files:**

- Modify: `src/agent/adapter_tests.rs`
- Modify: `src/agent/controller/tests.rs`
- Modify: `tests/agent_characterization.rs`
- Modify: `tests/opencode_smoke.rs`
- Modify: `docs/superpowers/plans/2026-08-06-brain-agent-facade-opencode.md`

- [x] **RED then GREEN: define the current frontend matrix.** Add one table-driven characterization contract covering Claude, Codex, and OpenCode for label, configured-command key, submit, busy-turn follow-up, new session, fresh/resume support, completion strategy, and receiver-resume support. Assert only behavior the adapters already implement in this task.
- [x] **RED then GREEN: prove callers use semantic operations.** Add source-boundary assertions or focused compile-time seams showing that TUI and receiver code do not construct frontend key sequences or command flags.
- [x] Record the false OpenCode assumptions as explicit cases owned by Tasks 3, 6, and 7. Introduce each failing assertion only when its owning task immediately implements the behavior, so every task and every commit ends with a green suite.
- [x] Make only the test harness and fixture changes required to characterize the current contract. Do not implement missing behavior in this task.
- [x] Mark the older facade/OpenCode plan as the historical foundation and link this gap-closure plan as its completion follow-up.

**Verification:**

```sh
cargo test --release agent::adapter_tests
cargo test --release agent::controller::tests
cargo test --release --test agent_characterization
cargo test --release --test opencode_smoke
```

**Exit criteria:** Every missing behavior maps to one explicit red-green slice later in this plan; the full suite remains green and no requirement exists only in prose.

---

## Task 2: Add a registry-backed frontend construction and integration boundary

**Files:**

- Modify or split: `src/agent/mod.rs`
- Create: `src/agent/registry.rs`
- Modify: `src/agent/controller/mod.rs`
- Modify: `src/tui/app_brain/launch.rs`
- Modify: `src/skills/command.rs`
- Modify: `src/command/server/receiver/hooks.rs`
- Modify: `src/tasks/doctor/mod.rs`

- [x] **RED: enumerate all functional frontends.** Add `AgentKind::ALL` and assert that it contains Claude, Codex, and OpenCode exactly once in stable display order.
- [x] **RED: test registry completeness.** Each kind must supply:
  - configured command resolution;
  - an `AgentFrontend` constructor;
  - lifecycle installation descriptors;
  - health-check descriptors;
  - capability-enforcement evidence;
  - display metadata.
- [x] **RED: reject fixed two-frontend rendering.** Update skills, setup, and doctor tests so a frontend omitted by the registry fails the test.
- [x] **GREEN: implement a small registry.** Keep concrete constructors private to `src/agent/`. Shared callers request a frontend or integration descriptor by `AgentKind`; they never import `ClaudeFrontend`, `CodexFrontend`, or `OpenCodeFrontend`.
- [x] Add `AgentController::configured(...)` or an equivalent facade constructor so TUI code does not assemble a concrete frontend and transport itself.
- [x] Keep transport injection available for tests without exposing adapter internals.
- [x] Remove the unused public `transcript()` facade operation. Preserve Claude transcript lookup as private resume-validation behavior unless another production caller is identified.

**Verification:**

```sh
cargo test --release agent::
cargo test --release skills::command::
cargo test --release tasks::doctor::
rg -n 'ClaudeFrontend|CodexFrontend|OpenCodeFrontend' src --glob '!src/agent/**'
```

**Exit criteria:** Concrete adapter types occur only under `src/agent/` and adapter-focused tests; shared setup, status, skills, TUI, and receiver code consume registry data.

---

## Task 3: Implement OpenCode executable and compatibility preflight

**Files:**

- Modify or split: `src/agent/opencode.rs`
- Create: `src/agent/opencode/probe.rs`
- Modify: `src/agent/frontend.rs`
- Modify: `src/agent/controller/tests.rs`
- Modify: `tests/opencode_smoke.rs`
- Modify: `src/tasks/doctor/mod.rs`

- [x] **RED: missing executable.** An OpenCode command whose executable cannot be resolved returns a themed, actionable error before state claim or PTY spawn.
- [x] **RED: incompatible command.** Probe output that lacks required TUI flags or the JSON session-list surface is rejected with the exact missing capability.
- [x] **RED: supported command.** Fixture output modeled on OpenCode `1.18.14` passes.
- [x] **RED: command wrappers.** A configured command containing ordinary wrapper flags is probed without treating the complete string as one executable path.
- [x] **GREEN: add an injectable preflight runner.** Run read-only, time-bounded `--version`, `--help`, and `session list --help` probes through the configured command's normal shell boundary. Capture bounded stdout/stderr and redact the command in diagnostics when it could contain local details.
- [x] Prefer feature detection over a hard minimum version. Record the parsed version for diagnostics, but accept compatible future versions.
- [x] Cache a successful probe for the current process and exact configured command so controller methods do not spawn a probe repeatedly.
- [x] Add an OpenCode preflight row to doctor without making doctor mutate config or install plugins.

**Verification:**

```sh
cargo test --release agent::opencode::probe::
cargo test --release --test opencode_smoke
cargo test --release --test doctor_integration
```

**Exit criteria:** Brain fails early and clearly when OpenCode is absent or incompatible, and compatible commands reach the transport exactly once.

---

## Task 4: Correct and harden the OpenCode lifecycle plugin

**Files:**

- Modify: `scripts/opencode_brain_plugin.js`
- Create: `tests/fixtures/opencode/plugin_harness.js`
- Add or modify: `tests/opencode_plugin.rs`
- Modify: `src/command/server/receiver/hooks/tests_parts/part_02.rs`

- [x] **RED: use the supported SDK call.** The harness must fail unless lookup uses the v1 plugin-client shape `client.session.messages({ path: { id: sessionID }, query: { directory } })`.
- [x] **RED: reject child sessions.** Cover:
  - root `session.created` with no `parentID`;
  - child `session.created` with `parentID`;
  - root and child `session.idle`, where idle contains only `sessionID`;
  - resumed root sessions whose creation event occurred before plugin startup.
- [x] On idle, resolve session metadata using `client.session.get({ path: { id: sessionID }, query: { directory } })` or a maintained root-session cache backed by an SDK lookup. Never infer root status from missing event fields.
- [x] **RED: select completion text safely.** Test no messages, user-only messages, thinking-only assistant output, tool-only output, multiple assistant messages, multiple text parts, ignored/synthetic text parts, an assistant error, and an incomplete assistant message.
- [x] Select the newest completed, non-error assistant message and join its eligible text parts deterministically.
- [x] **RED: deduplicate repeated idle.** Repeated idle events may invoke the normalized completion bridge, but must produce at most one response artifact through the bridge's state guard.
- [x] **RED: surface failures.** SDK lookup errors, malformed responses, and hook subprocess failures must call OpenCode's logging API with non-secret metadata. They must not emit completion.
- [x] **RED: safe subprocess environment.** Send JSON only through stdin. Pass the required `BRAIN_*` variables plus runtime necessities; never interpolate session IDs or assistant text into a shell command.
- [x] **GREEN:** implement the smallest plugin changes that satisfy the harness. Keep database and response publication logic out of JavaScript.
- [x] Preserve legacy named-function plugin loading for the supported OpenCode version, or export the current `PluginModule` shape with an explicit compatibility test.

**Verification:**

```sh
cargo test --release --test opencode_plugin
cargo test --release command::server::receiver::hooks::tests::install_adds_an_idempotent_opencode_brain_plugin
```

**Exit criteria:** Root OpenCode turns rotate and complete through the plugin; child sessions, failed lookups, and empty/error turns never publish a parent response.

---

## Task 5: Generalize the lifecycle bridge and migrate installations

**Files:**

- Create: `scripts/agent_session_start_hook.py`
- Create: `scripts/agent_turn_complete_hook.py`
- Modify or replace with compatibility launchers: `scripts/claude_session_start_hook.py`
- Modify or replace with compatibility launchers: `scripts/claude_stop_hook.py`
- Modify: `src/command/server/receiver/hooks.rs`
- Modify: `src/command/server/receiver/setup/transaction.rs`
- Modify: `src/command/server/receiver/setup/transaction/tests.rs`
- Modify: `tests/hook_integration.rs`
- Modify: `tests/stop_hook_actor.rs`

- [x] **RED: normalized start contract.** Accept `{ "session_id": "...", "source": "startup" }` for every frontend. Keep `thread_id` only as Codex compatibility input at the bridge boundary.
- [x] **RED: normalized completion contract.** Accept `{ "session_id": "...", "last_assistant_message": "..." }` for every frontend. Claude transcript fallback remains a Claude-only compatibility branch.
- [x] Test Claude, Codex, and OpenCode against exact workspace, actor, channel, frontend, instance, pending session, launch, and response identities.
- [x] Prove pending-to-real session rotation is atomic and cannot steal another live lineage.
- [x] Prove duplicate, stale, wrong-workspace, wrong-actor, child-session, and wrong-frontend events are no-ops.
- [x] Prove completion publication and DB status update remain one recoverable transaction when file publication or SQLite commit fails.
- [x] **GREEN: move generic logic into generic scripts.** Keep old filenames only as tiny compatibility launchers if installed frontends may still reference them during migration.
- [x] Update Claude settings, Codex hooks, and the OpenCode plugin to invoke generic bridges.
- [x] Installer matching must remove both old and new Brain-owned registrations without touching unrelated hooks.
- [x] Extend transaction snapshots and failure injection to every generic script, settings file, plugin, and created directory.

**Verification:**

```sh
cargo test --release --test hook_integration
cargo test --release --test stop_hook_actor
cargo test --release command::server::receiver::setup::transaction::tests
```

**Exit criteria:** All three frontends share normalized lifecycle scripts; old installations converge idempotently and rollback restores exact prior bytes.

---

## Task 6: Add workspace-scoped OpenCode session discovery and resume

**Files:**

- Create: `src/agent/opencode/session.rs`
- Modify: `src/agent/opencode.rs`
- Modify: `src/agent/frontend.rs`
- Modify: `src/tui/app_brain/launch.rs`
- Modify: `tests/opencode_smoke.rs`
- Modify: `src/tui/app_brain/tests/launch.rs`
- Add fixture: `tests/fixtures/opencode/session-list.json`

- [x] **RED: parse session-list JSON.** Cover empty arrays, malformed JSON, missing IDs, duplicate IDs, child sessions, wrong directories, archived/deleted sessions if represented, and unknown additive fields.
- [x] **RED: exact workspace match.** A session is resumable only when its ID exists as a root session for the selected workspace root. Normalize paths using the same canonical workspace context Brain already resolved.
- [x] **RED: stale Brain row.** A missing OpenCode session is skipped and Brain falls back to the next valid candidate or a fresh placeholder, with the existing user-visible resume-fallback notice.
- [x] **RED: receiver resume.** `can_resume_response_session` must depend on the exact completed session candidate, not a frontend-wide boolean. Refactor the facade method to accept an `AgentSession` if necessary.
- [x] **RED: read-only behavior.** Session discovery must not create, update, export, delete, or resume an OpenCode session.
- [x] **GREEN: execute `opencode session list --format json`** through an injectable, bounded runner with cwd set to the selected workspace. Keep subprocess and JSON details private to the adapter.
- [x] Cache one discovery snapshot only for the duration of one launch decision. Do not retain stale results across later panel opens.
- [x] Keep `opencode export` out of the normal resume path. Use it only if a future production caller genuinely needs transcript content.

**Verification:**

```sh
cargo test --release agent::opencode::session::
cargo test --release --test opencode_smoke
cargo test --release tui::app_brain::tests::launch::
```

**Exit criteria:** Brain resumes only a real root OpenCode session in the selected workspace, and stale rows degrade safely to a fresh launch.

---

## Task 7: Define real semantic submit, follow-up, and new-session behavior

**Files:**

- Modify: `src/agent/frontend.rs`
- Modify: `src/agent/controller/mod.rs`
- Modify: `src/agent/claude.rs`
- Modify: `src/agent/codex.rs`
- Modify: `src/agent/opencode.rs`
- Modify: `src/tui/app_brain/launch.rs`
- Modify: `src/tui/handlers/input.rs`
- Modify: `src/agent/controller/tests.rs`
- Modify: `src/tui/app_brain/tests/launch.rs`

- [x] Replace byte-oriented frontend methods with one semantic action translation if that keeps the API smaller:

```rust
enum AgentAction<'a> {
    TypeText(&'a str),
    SubmitNow,
    FollowUpAfterActiveTurn(&'a str),
    StartNewSession,
}
```

- [x] **RED: distinguish submit and follow-up.** An already-open panel receiving a task prompt must use the frontend's documented busy-turn follow-up behavior. A user pressing Enter must use immediate submit.
- [x] **RED: remove timer guessing.** Assert that correctness does not depend on two arbitrary event-loop ticks. If OpenCode's Enter behavior provides native queueing while busy, encode that explicitly in the adapter contract. If it does not, hold the prompt in controller state until a frontend lifecycle-idle signal is observed.
- [x] **RED: queue replacement/order.** Define behavior for two queued follow-ups, panel shutdown with a queued follow-up, and starting a new session while a follow-up is pending.
- [x] **RED: OpenCode new session.** `/new` plus Enter creates a new frontend session, and the start bridge rotates the Brain lineage to the new real ID.
- [x] Preserve literal typing and raw terminal forwarding without allowing shared callers to construct semantic frontend sequences.
- [x] **GREEN:** implement the smallest controller state machine and adapter translations needed by the tests.

**Verification:**

```sh
cargo test --release agent::controller::tests
cargo test --release tui::app_brain::tests::launch::
cargo test --release tui::app_brain::tests::lifecycle::
```

**Exit criteria:** Submit, busy-turn follow-up, and new session have explicit, tested semantics for every frontend; no arbitrary timer stands in for frontend state.

---

## Task 8: Preserve OpenCode configuration and prepare capabilities

**Files:**

- Create: `src/agent/opencode/config.rs`
- Modify: `src/agent/opencode.rs`
- Modify: `src/access/mod.rs`
- Modify or split: `src/access/mcp/`
- Modify: `tests/agent_access_adapter.rs`
- Modify: `tests/workspace_capabilities.rs`
- Modify: `tests/opencode_smoke.rs`

- [x] **RED: merge valid inherited inline config.** Preserve unrelated top-level keys, agents, commands, MCPs, permissions, and plugins. Brain owns only its reserved `brain` agent and reserved capability entries.
- [x] **RED: reject malformed inherited inline config.** Return a clear prelaunch error without replacing the user's value or spawning OpenCode.
- [x] **RED: trusted prompt separation.** Brain's workspace/actor policy appears only in the reserved agent prompt; untrusted initial text remains only in `--prompt`.
- [x] **RED: named-agent stability.** `--agent brain`, `/new`, and resumed sessions continue using the Brain agent without deleting other user agents.
- [x] **RED: translate selected MCPs.** Cover local command/args/env, remote URL/headers, missing credentials, excluded MCPs, and secret redaction in diagnostics.
- [x] **RED: selected skills.** Use the actor-specific rendered skill directory and documented OpenCode permission/config surfaces where possible. Do not delete or rewrite the user's global skills.
- [x] **RED: honest enforcement.** Report `StrictlySelected` only if tests prove unselected global/project sources are unavailable. Otherwise report `AdvisoryOnly` and document why.
- [x] **GREEN:** serialize one merged `OPENCODE_CONFIG_CONTENT` value. Keep credentials in environment or owner-only UUID-scoped artifacts, never argv or logs.
- [x] Add cleanup and rollback for any capability artifacts to the controller-owned launch lifecycle.

**Verification:**

```sh
cargo test --release --test agent_access_adapter
cargo test --release --test workspace_capabilities
cargo test --release --test opencode_smoke
```

**Exit criteria:** OpenCode receives Brain's trusted agent and selected capabilities without erasing unrelated user configuration or overstating isolation.

---

## Task 9: Unify CLI selection and conflicts

**Files:**

- Modify: `src/cli/global.rs`
- Modify: `src/command/tasks.rs`
- Modify: `src/command/dispatch.rs`
- Modify: `tests/opencode_smoke.rs`
- Modify: `tests/workspace_help_contract.rs` if help snapshots change

- [x] **RED: test every supported position.** Cover bare `brain`, `brain tasks`, delegated task grammar, selectors before and after task actions, long flags, aliases, and `--` option termination.
- [x] **RED: test all conflicts.** `--codex` plus `--open-code`, aliases mixed with long forms, duplicate different selectors, and selectors split between global and delegated positions must all return the same typed conflict before workspace bootstrap.
- [x] Duplicate selectors for the same frontend may be accepted or rejected, but the decision must be explicit and tested.
- [x] **GREEN: replace `take_agent_flag`.** Use one pure selector-extraction and validation function shared by global and delegated parsing. It returns the selected kind plus remaining arguments without last-flag-wins behavior.
- [x] Preserve values after `--` verbatim so prompt/task text resembling a selector is never consumed.
- [x] Keep help text for `--open-code` and `-oc` aligned across command surfaces.

**Verification:**

```sh
cargo test --release cli::global::tests
cargo test --release command::tasks::tests
cargo test --release --test opencode_smoke
```

**Exit criteria:** One frontend-selection decision governs every launch path and every conflict fails before side effects.

---

## Task 10: Make installation, doctor, and status registry-driven

**Files:**

- Modify or split: `src/command/server/receiver/hooks.rs`
- Modify: `src/command/server/receiver/setup/transaction.rs`
- Modify: `src/tasks/doctor/mod.rs`
- Modify: `src/skills/command.rs`
- Modify: `tests/doctor_integration.rs`
- Modify: `tests/status_read_only_parts/part_02.rs`

- [x] **RED: descriptor-driven installation.** Every registry frontend contributes zero or more Brain-owned artifacts; shared code performs locking, atomic writes, permissions, snapshots, and rollback.
- [x] **RED: preserve unrelated state.** Existing OpenCode plugins and config, Claude hooks, Codex hooks, malformed files, symlinks, and ownership-sensitive paths retain the repository's current safety behavior.
- [x] **RED: all-step rollback.** Inject failure after every script, settings, plugin, and directory step and compare exact before/after snapshots.
- [x] **RED: health semantics.** OpenCode health requires compatible executable, installed loadable plugin, generic bridges, and any required capability schema support. A file merely existing is not sufficient if its content is stale.
- [x] **RED: read-only status.** Doctor and status must not refresh hooks, create plugin directories, run OpenCode in a mutating mode, or update OpenCode state.
- [x] **GREEN:** replace fixed `claude_hook_installed`, `codex_hook_installed`, and `opencode_plugin_installed` fields with frontend health rows while preserving attractive deterministic rendering.
- [x] Render capability evidence for Claude, Codex, and OpenCode from the same registry.

**Verification:**

```sh
cargo test --release command::server::receiver::hooks::tests
cargo test --release command::server::receiver::setup::transaction::tests
cargo test --release --test doctor_integration
cargo test --release --test status_read_only
```

**Exit criteria:** Adding a future frontend requires one registry entry and adapter-owned descriptors, not new setup/doctor/status match branches.

---

## Task 11: Add full OpenCode TUI, receiver, and triage acceptance coverage

**Files:**

- Create: `tests/fixtures/opencode/fake-opencode`
- Create or expand: `tests/opencode_acceptance.rs`
- Modify: `src/tui/app_brain/tests/fixtures.rs`
- Modify: `src/tui/app_brain/tests/launch.rs`
- Modify: `src/tui/app_brain/tests/lifecycle.rs`
- Modify: `src/tui/app_brain/tests/receiver.rs`
- Modify: `src/tui/app_brain/tests/receiver_sync.rs`
- Modify: `src/tui/app_brain/tests/triage.rs`
- Modify: `tests/agent_characterization.rs`

- [x] Build a focused fake executable that can record bounded argv, cwd, non-secret environment names, and PTY input; emit fixture version/help/session-list output; stay alive; and terminate on request.
- [x] Build a plugin harness that executes the actual installed plugin source with a fake SDK client and fake generic hook process.
- [x] **RED and GREEN one behavior at a time:**
  1. `brain --open-code` fresh launch.
  2. `brain -oc` fresh launch.
  3. Initial prompt separation.
  4. Literal typing without submission.
  5. Immediate submission.
  6. Busy-turn follow-up.
  7. `/new` and pending-to-real ID rotation.
  8. Root completion artifact with actor/channel attribution.
  9. Child session ignored.
  10. Repeated idle deduplicated.
  11. Valid resume after restart.
  12. Missing session fallback to fresh.
  13. Authenticated receiver launch and response delivery.
  14. Receiver follow-up resume only when the session exists.
  15. Triage ephemeral launch and cleanup.
  16. Agent exit closes only the relevant panel.
  17. TUI close shuts down each OpenCode transport exactly once.
  18. Missing executable, incompatible CLI, plugin error, and malformed config diagnostics.
- [x] Expand every frontend-neutral TUI and receiver loop that currently lists only Claude and Codex to include OpenCode where the contract is meant to be shared.
- [x] Add one opt-in local compatibility script that uses the installed OpenCode executable and a no-provider/no-prompt path to verify plugin loading and CLI probes. Keep paid/provider calls manual and outside CI.

**Verification:**

```sh
cargo test --release --test opencode_acceptance
cargo test --release tui::app_brain::tests
cargo test --release --test agent_characterization
cargo test --release --test receiver_workspace_isolation
```

**Exit criteria:** OpenCode passes the same application-level lifecycle contract as Claude and Codex without a live LLM dependency.

---

## Task 12: Correct all current documentation

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

- [x] Remove current-behavior claims that OpenCode is a stub, inert, unsupported, or outside health checks. Leave historical plans/specs truthful about their original phase.
- [x] Document the common facade, registry, semantic actions, availability probes, and session-discovery contract.
- [x] Document exact OpenCode launch flags, supported feature probes, config precedence, merge ownership, plugin location, event translation, root-session validation, generic lifecycle scripts, and receiver delivery.
- [x] Document the difference between native busy-turn follow-up and controller-held follow-up if frontends differ.
- [x] Document OpenCode session-list resume validation and stale-row fallback.
- [x] Document capability evidence honestly and state what remains advisory.
- [x] Update doctor/status and testing sections to describe real compatibility checks and fake-process coverage.
- [x] Record why OpenCode plugin code is thin and why DB authorization remains in Brain's generic bridge.
- [x] Add source links and the supported-feature policy without promising permanent compatibility with every future OpenCode release.

**Audit:**

```sh
rg -n 'OpenCode.*(stub|inert|fail-fast|unsupported|not supported)|opencode.*(stub|inert)' \
  AGENTS.md docs src tests \
  --glob '!docs/superpowers/plans/2026-08-02-*' \
  --glob '!docs/superpowers/specs/**'
rg -n 'claude_session_start_hook|claude_stop_hook' AGENTS.md docs src scripts tests
```

**Exit criteria:** Current docs agree with tested behavior; every surviving historical stub reference is intentionally historical.

---

## Task 13: Version, live compatibility check, and release verification

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify any source or docs identified by the final audits

- [x] Bump Brain from `0.37.0` to `0.38.0` because complete OpenCode support is an additive user-visible feature. Regenerate `Cargo.lock` through Cargo.
- [x] Run focused suites after each red/green slice; then run:

```sh
cargo fmt --all -- --check
cargo test --release agent::
cargo test --release --test agent_characterization
cargo test --release --test agent_access_adapter
cargo test --release --test opencode_smoke
cargo test --release --test opencode_plugin
cargo test --release --test opencode_acceptance
cargo test --release --test hook_integration
cargo test --release --test stop_hook_actor
cargo test --release --test doctor_integration
cargo test --release --test status_read_only
cargo test --release tui::app_brain::tests
cargo test --release bundled_skills_carry_no_personal_data
cargo test --release
cargo clippy --release --all-targets -- -D warnings
```

- [x] Build and run non-TUI smoke checks:

```sh
cargo build --release
./target/release/brain --help
./target/release/brain env get opencode_cmd
./target/release/brain workspace list
./target/release/brain server status
./target/release/brain tasks today --no-tui
```

- [x] Run the opt-in local compatibility probe against the installed OpenCode version. It may check `--version`, required help flags, JSON session-list parsing, and plugin loading. It must not send an LLM prompt or alter provider authentication.
- [x] Manually exercise one fresh OpenCode panel, one submitted message, `/new`, panel restart/resume, and one authenticated receiver fixture in a disposable test workspace. Verify the local response artifact and state transition only; do not send email or SMS. Record only pass/fail and redacted IDs, never prompt or response contents.
- [x] Run final branch audits:

```sh
rg -n 'AgentKind::(Claude|Codex|OpenCode)|ClaudeFrontend|CodexFrontend|OpenCodeFrontend' src
rg -n 'take_agent_flag|validate_agent_kind' src tests
rg -n 'path:.*id|query:.*directory' scripts tests
git diff --check
git status --short
```

- [x] Inspect production, test, fixture, and script modules for cohesive ownership and the repository's approximate 400-line smell threshold.
- [x] Review changed text for secrets, personal paths, and unnecessary em dashes.
- [x] After all verification and live acceptance pass, follow the public brain-core workflow for commit and push. Do not publish a claim of full OpenCode support if the live lifecycle check remains unverified.

**Exit criteria:** Brain `0.38.0` has complete automated parity coverage, passes the real installed OpenCode compatibility probe, and has one successful end-to-end OpenCode lifecycle acceptance run.

---

## Final acceptance criteria

### Facade

- `AgentController` is the sole application-facing interface for launch, text entry, submit, busy-turn follow-up, new session, session validation, response identity, completion, receiver resume, terminal control, and shutdown.
- TUI, receiver, skills, setup, doctor, and status code do not import concrete adapter types or construct frontend commands/key sequences.
- Every functional frontend is present in one registry and one shared contract suite.

### OpenCode

- `--open-code` and `-oc` work in every supported global and delegated position, with deterministic conflict handling.
- Missing or incompatible OpenCode fails before state claims and PTY spawn.
- Fresh launch, initial prompt, literal typing, submit, busy-turn follow-up, `/new`, clean shutdown, and panel restart work.
- Pending Brain IDs rotate to real OpenCode root-session IDs.
- Resume validates exact root sessions in the selected workspace and safely skips stale rows.
- Child agents cannot rotate the panel lineage or publish the panel's completion.
- Root idle completion retrieves the final eligible assistant text through the supported SDK and delivers at most one attributed response.
- Receiver and triage paths have the same lifecycle guarantees as the main interactive panel.

### Configuration and installation

- Brain preserves unrelated OpenCode inline, global, and project configuration.
- Brain owns only its reserved agent, capability entries, project plugin, and generic lifecycle bridge registrations.
- Installation is idempotent and transactionally recoverable without deleting unrelated user files.
- Doctor and status are read-only and report executable, plugin, bridge, session, and capability health accurately.
- MCP and skill enforcement is labeled strict only when tests prove exclusion; otherwise it remains advisory.

### Quality

- Every production behavior was preceded by a focused test observed failing for the intended reason.
- No automated test calls a paid or remote LLM.
- OpenCode `1.18.14` or a feature-compatible installed version passes the local compatibility probe.
- One disposable-workspace end-to-end lifecycle run has passed before release claims full support.
- Formatting, the full release suite, strict Clippy, documentation audits, and the bundled-skill personal-data guard are green.
