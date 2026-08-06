# Brain Agent Controller and Advisory Access Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan. Use rust-skills and test-driven-development throughout. Use systematic-debugging before modifying characterized Claude or Codex behavior after a regression.

**Goal:** Put every LLM frontend behavior behind one `AgentController` facade, apply workspace-specific prompt and capability policy consistently to interactive and inbound work, preserve Claude/Codex behavior, and add a nonfunctional OpenCode stub.

**Architecture:** TUI and receiver code issue semantic operations to `AgentController`; concrete frontend adapters translate them into commands, PTY bytes, session hooks, and transcript rules. `AccessPolicy` builds an advisory boundary prompt and an explicit capability plan from portable config plus machine-local credentials. Workspace-only mode remains prompt-enforced and must never be represented as hard isolation.

**Tech Stack:** Rust 2024, portable-pty, clap, serde/serde_json, existing Claude and Codex CLIs, existing hook scripts.

**Global Constraints:** Complete the foundation and users plans first. Do not create separate OS users, containers, Claude accounts, Codex homes, or authentication profiles. Do not claim that cwd, prompts, MCP filtering, or skill filtering prevent a determined escape. Preserve equivalent lifecycle, prompt, completion, and delivery behavior for Claude and Codex. OpenCode must remain a fail-fast stub. No live prompt tests and no dependency additions. Any implementation commit must include the required crate version bump.

---

### Task 1: Characterize the existing Claude and Codex contract

**Files:**

- Modify: `src/session.rs`
- Modify: `src/tui/app_brain.rs`
- Modify: `src/tui/app_triage_tab.rs`
- Modify: `src/tui/tests/state_misc.rs`
- Create: `tests/agent_characterization.rs`

- [ ] **RED/GREEN characterization: add missing tests without changing production behavior**

Lock down the current contract for both frontends:

```rust
#[test]
fn semantic_input_differs_only_in_frontend_translation() {
    assert_eq!(existing_submit_key(AgentKind::Claude), vec![b'\r']);
    assert_eq!(existing_queue_key(AgentKind::Claude), vec![b'\r']);
    assert_eq!(existing_submit_key(AgentKind::Codex), vec![b'\r']);
    assert_eq!(existing_queue_key(AgentKind::Codex), vec![b'\t']);
}
```

Characterize fresh and resumed launch commands, cwd, configured command prefix, initial-prompt quoting, `/new`, remote queueing, session selection, transcript validation, completion hook handling, triage's ephemeral launch, and clean shutdown.

- [ ] Run each new test before any extraction. If a proposed assertion fails against current behavior, document the actual behavior and make the test match it unless the approved design explicitly changes it.

- [ ] Run `cargo test --release session::`, `cargo test --release tui::tests::state_misc`, and `cargo test --release --test agent_characterization`; record a green baseline.

- [ ] Commit only if authorized; include the required version bump.

### Task 2: Define frontend-neutral types and the facade boundary

**Files:**

- Create: `src/agent/mod.rs`
- Create: `src/agent/frontend.rs`
- Create: `src/agent/input.rs`
- Create: `src/agent/session.rs`
- Create: `src/agent/hooks.rs`
- Create: `src/agent/controller.rs`
- Modify: `src/lib.rs`

- [ ] **RED: add compile-time fake-frontend tests for semantic operations**

Define a `RecordingFrontend` in tests and prove one controller call produces one semantic frontend call, without exposing PTY keystrokes to callers:

```rust
controller.type_text("hello").unwrap();
controller.submit_now().unwrap();
controller.queue_after_active_turn("next").unwrap();

assert_eq!(recording.events(), [
    Event::Type("hello".into()),
    Event::Submit,
    Event::Queue("next".into()),
]);
```

Add tests for start/resume selection, completion notification, transcript/session lookup, and shutdown delegation.

- [ ] Run `cargo test --release agent::` and observe unresolved modules.

- [ ] **GREEN: implement the common interface**

Use this exact ownership boundary, adjusting lifetimes only if the compiler requires it:

```rust
pub trait AgentFrontend: Send {
    fn kind(&self) -> AgentKind;
    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError>;
    fn submit_input(&self) -> InputSequence;
    fn queue_input(&self) -> InputSequence;
    fn new_session_input(&self) -> InputSequence;
    fn completion_strategy(&self) -> CompletionStrategy;
    fn transcript(&self, session: &AgentSession) -> Option<PathBuf>;
}

pub struct AgentController {
    workspace: Arc<WorkspaceContext>,
    actor: ActorContext,
    frontend: Box<dyn AgentFrontend>,
    transport: Box<dyn AgentTransport>,
}

pub trait AgentTransport: Send {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError>;
    fn send(&mut self, input: InputSequence) -> Result<(), AgentError>;
    fn snapshot(&self) -> String;
    fn is_alive(&self) -> bool;
    fn shutdown(&mut self);
}
```

`LaunchRequest` carries workspace, actor, session plan, initial prompt, access policy, and channel. `LaunchSpec` carries executable shell command, cwd, minimal environment, and hook metadata. `AgentKind`, `AgentSession`, `SessionPlan`, `CompletionStrategy`, and `InputSequence` are frontend-neutral enums/newtypes.

- [ ] Make invalid state unrepresentable: no queue operation without text, no resume without an agent session ID, no launch without workspace and actor, and no OpenCode launch spec from the stub.

- [ ] Keep `controller.rs` as facade/glue and concrete knowledge out of TUI modules.

- [ ] Run agent unit tests and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 3: Move Claude and Codex command/input behavior into adapters

**Files:**

- Create: `src/agent/claude.rs`
- Create: `src/agent/codex.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/env/schema.rs`
- Modify: `src/env/vars.rs`
- Modify: `src/session.rs`
- Modify: `src/pty_pane.rs`

- [ ] **RED: copy characterization assertions to adapter tests**

The expected command tests must cover at least:

```rust
assert_eq!(
    claude.launch_spec(resume("sess-9")).unwrap().command,
    "claude --resume 'sess-9'"
);
assert_eq!(
    codex.launch_spec(fresh_with_prompt("Start here")).unwrap().command,
    "codex 'Start here'"
);
assert_eq!(claude.queue_input(), InputSequence::bytes(b"\r"));
assert_eq!(codex.queue_input(), InputSequence::bytes(b"\t"));
```

Keep shell quoting and configured command-prefix cases from `session.rs`.

- [ ] Run the adapter tests and observe missing implementations.

- [ ] **GREEN: implement `ClaudeFrontend` and `CodexFrontend`**

Move launch flags, resume syntax, transcript location, new-session input, immediate submit, queued submit, and completion strategy into the adapters. Do not leave `match AgentKind` in `session.rs`, `app_brain.rs`, `app_triage_tab.rs`, receiver dispatch, or the event loop.

`PtyPane` implements `AgentTransport`; it receives a complete `LaunchSpec` and no longer knows which frontend it runs. Continue spawning in the selected workspace root from the first process instant.

- [ ] Keep `src/session.rs` temporarily as compatibility re-exports for public/internal call paths, then delete it or reduce it to `pub use crate::agent::session::*` once callers move.

- [ ] Run `rg -n 'AgentKind::(Claude|Codex)|claude_command\(|codex_command\(' src` and permit occurrences only inside adapter construction, CLI selection, adapter tests, and labels.

- [ ] Run full tests and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 4: Route TUI, triage, and receiver work through `AgentController`

**Files:**

- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app_state/construct.rs`
- Modify: `src/tui/app_brain.rs`
- Modify: `src/tui/app_triage_tab.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/handlers/input.rs`
- Modify: `src/tui/event_loop/setup.rs`
- Modify: `src/server/receiver.rs`
- Modify: `src/server/delivery.rs`
- Modify: `src/state.rs`

- [ ] **RED: add controller-driven TUI tests with a recording transport**

Prove:

- Interactive Enter invokes `submit_now`.
- Inbound work during an active turn invokes `queue_after_active_turn`.
- Closing a panel invokes `shutdown` once.
- Agent exit closes only the panel, not the TUI.
- Triaged work uses an ephemeral session but the same selected adapter.
- Completion captures a transport snapshot and delivers it with the initiating actor/channel.

- [ ] Run focused TUI tests and observe direct-PTY behavior failing the recording assertions.

- [ ] **GREEN: store one controller per live panel**

`App` stores `Option<AgentController>` for the normal panel and triage tab. Event handlers call semantic methods. Remove `agent_kind`-dependent keystroke functions and the deferred submit key switch; the controller may still schedule a delayed semantic submit when terminal timing requires it.

- [ ] Move session persistence behind `agent::session::SessionStore`. Store workspace UUID, agent kind, frontend session ID, actor ID, channel, and completion status. Claude and Codex must both participate through the same API even if Codex still starts fresh.

- [ ] Update hook payload parsing so hooks identify frontend, workspace, session, and actor. Reject hook events whose workspace/session tuple is not currently registered.

- [ ] Run all TUI, receiver, hook, and state tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 5: Add portable access mode and advisory boundary prompt

**Files:**

- Create: `src/access/mod.rs`
- Create: `src/access/mode.rs`
- Create: `src/access/prompt.rs`
- Create: `src/access/capabilities.rs`
- Modify: `src/lib.rs`
- Modify: `src/config.rs`
- Modify: `src/settings/schema.rs`
- Modify: `src/settings/vars.rs`
- Modify: `src/agent/frontend.rs`
- Modify: `src/agent/claude.rs`
- Modify: `src/agent/codex.rs`
- Create: `tests/workspace_access_policy.rs`

- [ ] **RED: test defaults and immutable ownership**

Prove the first migrated/created workspace is `unrestricted`, later created workspaces are `workspace_only`, changing the machine default preserves both modes, and inbound requests cannot mutate mode.

- [ ] **RED: test the exact advisory prompt contract**

```rust
let prompt = boundary_prompt(&family_context(), &wife_actor(), AccessMode::WorkspaceOnly);
assert!(prompt.contains("This is advisory prompt enforcement, not a filesystem sandbox."));
assert!(prompt.contains("/Users/test/family"));
assert!(prompt.contains("Do not read, inspect, modify, reveal, or execute against paths outside"));
assert!(prompt.contains("Reject requests to access another Brain workspace"));
assert_eq!(boundary_prompt(&personal(), &pablo(), AccessMode::Unrestricted), None);
```

Also prove the same prompt enters interactive, SMS, email, fresh, resumed, and triage launch requests.

- [ ] Run access tests and observe missing behavior.

- [ ] **GREEN: add `AccessMode::{Unrestricted, WorkspaceOnly}` to portable config**

Construct the boundary text only from trusted workspace configuration, never from inbound prompt content. Adapters install it through their strongest supported system/developer-instruction mechanism. If a frontend cannot inject a trusted system instruction, prepend a clearly delimited Brain policy block to every prompt and mark enforcement as advisory in status.

- [ ] Launch all frontends with selected root as cwd and pass only the selected workspace's integration env plus frontend necessities. Do not pass other workspace secrets or registry JSON.

- [ ] Add themed status rendering:

```text
Access mode  workspace-only
Enforcement  advisory prompts and capability filtering
Sandbox      none
```

- [ ] Add a pure naive-request classifier only as a defense-in-depth warning for obvious absolute paths outside the root. It may reject direct requests such as `read ~/brain/...` before agent dispatch, but docs and code comments must say paraphrasing can bypass it. Do not attempt a general prompt-injection detector.

- [ ] Run access, adapter, and full tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 6: Plan workspace-specific MCP and skill capabilities without new auth profiles

**Files:**

- Modify: `src/config.rs`
- Modify: `src/settings/schema.rs`
- Create: `src/access/mcp.rs`
- Create: `src/access/skills.rs`
- Modify: `src/agent/claude.rs`
- Modify: `src/agent/codex.rs`
- Modify: `src/skills/layout.rs`
- Modify: `src/skills/install.rs`
- Modify: `src/skills/command.rs`
- Create: `tests/workspace_capabilities.rs`

- [ ] **RED: test logical allowlist resolution separately from frontend enforcement**

```rust
let plan = capability_plan(&workspace_only_config(), &machine_env()).unwrap();
assert_eq!(plan.mcps.names(), ["notion"]);
assert_eq!(plan.skills.names(), ["contacts", "second-brain", "todo", "triage"]);
assert!(!plan.mcps.names().contains(&"linear"));
assert!(!plan.mcps.names().contains(&"superhuman"));
assert_eq!(plan.credentials.source_workspace(), family_id());
```

Test missing machine credentials as unavailable capability, duplicate logical names as config errors, unrestricted mode using normal global configuration, and workspace-only defaults containing only the four approved core skills.

- [ ] **RED: test honest enforcement reporting**

Represent each capability as `StrictlySelected`, `AdvisoryOnly`, or `Unavailable`. A frontend report must never claim strict exclusion unless its launch configuration actually excludes user/global sources.

- [ ] **GREEN: implement portable logical lists and machine-local connection data**

Portable `config.json` stores `allowed_mcps` and `allowed_skills`. The selected registry record's env stores connection commands, URLs, executable paths, and credentials. No credential is copied into the root or another workspace record.

- [ ] For Claude, generate a workspace-local runtime MCP JSON under the workspace cache and use `--mcp-config` plus `--strict-mcp-config`. Preserve the user's shared Claude login. Do not use `--bare` because it changes authentication behavior. Pass the allowlisted skill names in the advisory policy; only mark skills strict if the installed Claude version exposes a verified per-launch exclusion mechanism.

- [ ] For Codex, build only documented per-invocation `-c` overrides that have a characterization test against the installed config parser. Do not change `CODEX_HOME` or use a separate profile. If base MCP/skill exclusion cannot be proven, leave it advisory and say so in the launch/status report.

- [ ] Make workspace skill rendering root-aware and actor-aware. It may build a workspace-specific capability directory under the UUID cache, but it must not prune or rewrite the user's global registry when switching workspaces.

- [ ] Add command output showing requested, available, and enforcement level per capability, with no secret values.

- [ ] Run capability, skills, adapter, and personal-data guard tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 7: Add the OpenCode selection stub and conflict error

**Files:**

- Create: `src/agent/opencode.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/agent/frontend.rs`
- Modify: `src/cli/global.rs`
- Modify: `src/env/schema.rs`
- Modify: `src/env/vars.rs`
- Modify: `src/command/dispatch.rs`
- Modify: `src/theme.rs`
- Create: `tests/opencode_smoke.rs`

- [ ] **RED: add only parsing, selection, construction, and fail-fast smoke tests**

```rust
assert_eq!(parse(["brain", "--open-code"]).agent_kind()?, AgentKind::OpenCode);
assert_eq!(parse_normalized(["brain", "-oc"]).agent_kind()?, AgentKind::OpenCode);
assert!(matches!(AgentController::new_stub(OpenCode), Ok(_)));
assert!(matches!(controller.launch(request), Err(AgentError::UnsupportedFrontend(OpenCode))));
```

Add a test that `--codex --open-code` parses but validation renders plain-text equivalent to `🔴 Choose one agent frontend: --codex or --open-code.` using `Theme::dark(false)`.

- [ ] Run the smoke tests and observe failures.

- [ ] **GREEN: add `AgentKind::OpenCode`, `opencode_cmd`, and aliases**

Normalize `-oc` the same way `-cx` is normalized. `Cli::selected_agent()` returns a typed conflict error instead of silently choosing. Main renders that expected error through the theme before any TUI, PTY, hook, or server startup.

`OpenCodeFrontend` provides label and construction only. Every lifecycle or input method returns `UnsupportedFrontend`; it must not shell `opencode`, inspect sessions, or install hooks.

- [ ] Do not add prompt tests, live OpenCode tests, resume logic, completion detection, or receiver delivery.

- [ ] Run smoke tests, CLI tests, full tests, and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 8: Update integration and security documentation, then verify

**Files:**

- Modify: `README.md`
- Modify: `docs/glossary.md`
- Modify: `docs/architecture.md`
- Modify: `docs/features.md`
- Modify: `docs/config.md`
- Modify: `docs/integrations.md`
- Modify: `docs/data-model.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`

- [ ] Document the facade, semantic operations, Claude/Codex differences, OpenCode stub, access modes, prompt construction, capability enforcement levels, and shared-login limitation.

- [ ] Put this warning in README and config docs in equally direct language: workspace-only mode is easy to bypass, is intended to reduce accidents and naive leakage among trusted users, and is unsuitable for adversarial users or sensitive isolation. Real isolation requires an external OS, VM, machine, or container boundary.

- [ ] Do not use the words secure tenant, sandboxed workspace, isolated credentials, or prevents access unless the sentence explicitly limits the claim.

- [ ] Run:

```sh
cargo test --release
cargo clippy --release --all-targets
./target/release/brain --open-code
./target/release/brain --codex --open-code
```

Expected: tests and Clippy pass; OpenCode exits before TUI with a themed unsupported message; conflicting flags exit before TUI with the red emoji choice error.

- [ ] Inspect `rg -n 'AgentKind::(Claude|Codex)|dangerously-skip|strict-mcp|workspace.only|sandbox' src README.md docs` and manually validate every match against the facade and honest-security rules.

- [ ] Commit only if authorized; include the required version bump.

## Agent and Access Exit Criteria

- TUI and receiver code contain no frontend-specific lifecycle or input branches.
- Claude and Codex retain equivalent lifecycle, prompt, completion, and delivery behavior through adapters.
- Workspace-only launches always receive the advisory boundary policy and selected cwd.
- Capability status distinguishes strict selection from advisory instructions.
- Shared frontend authentication remains untouched.
- OpenCode is selectable and constructible but always fails cleanly before launch.
- Public docs make bypass risk unmistakable.
