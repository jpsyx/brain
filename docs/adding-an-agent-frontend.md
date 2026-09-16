# Adding an agent frontend

This is the contract for teaching `brain` to drive a new interactive coding
agent in the brain panel. Read [glossary.md](glossary.md) for the
main-view / brain-panel vocabulary and [integrations.md](integrations.md) for
how the existing frontends are wired before you start.

`brain` supports Claude Code, OpenAI Codex, OpenCode, and pi. Every one of them
reaches the application through the same facade: application code only ever
holds an `AgentController`, and a **frontend adapter** translates neutral
operations into that agent's own launch flags, keystrokes, session files, and
lifecycle events. Nothing outside `src/agent/` may name a concrete adapter
(`tests/agent_registry_boundary.rs` enforces this).

> **House rule.** Every LLM capability must flow through `AgentController` and
> work with **every** registered frontend. Adding a frontend means adding it to
> the whole contract, not adding a special case beside it.

---

## 0. Establish the frontend's facts first

Do not write code until you can answer all of these from the agent's own
documentation **and** from the binary installed on this machine. Record the
answers, with the version you verified them against, in the spec you write for
the change (see `docs/superpowers/specs/`). Every existing frontend's plan did
this, and every gap found later came from an unanswered question here.

| Question | Why brain needs it | Where it lands |
| --- | --- | --- |
| How do you launch it with a **caller-chosen session id**? | Brain, not the agent, owns session identity, so a launch can be reconnected to the right row in the state DB | `command_for` |
| How do you **resume** a specific session? | `SessionPlan::Resume` | `command_for` |
| How do you pass an **initial prompt** that still leaves the agent interactive? | Tasks-view actions and receiver launches seed the conversation | `command_for` |
| How do you **append to the system prompt**? | `AccessPolicy::boundary_prompt` | `launch_spec` |
| Where does it store sessions on disk, and how is the path derived from the cwd? | Resume eligibility must be proven, never guessed | `resume_candidate_exists` |
| What are its **keystrokes** for submit, for a follow-up while a turn is running, and for starting a new conversation? | `AgentAction` | `input_for` |
| What **lifecycle events** can it report to an external process (session started, turn finished, prompt submitted, tool finished)? | Session rotation, completion publication, receiver observation | the bridge artifact |
| How is that event integration **installed** (a settings file, a project plugin, a CLI-loaded extension)? | `LifecycleInstallation` | `registry/contract.rs` |
| Does it have a **project trust** or approval prompt that would block an unattended panel? | A blocked panel looks like a hang | `command_for` |
| Does it support **MCP**? Does it support **skills**, and can the selection be made exact? | Honest capability enforcement | `capability_evidence` |
| Which **environment variables** configure it, and which of them are per-session metadata that must *not* be inherited? | `launch_environment` passes a deliberately minimal environment | `ambient_frontend_environment` |
| What is the **minimum version** that has all of the above? | `ensure_available` must fail before the PTY spawns, not after | the compatibility probe |

Verify by running the real binary (`--help`, `--version`, a throwaway session in
a scratch directory). Reading the docs is not enough: `--help` on the installed
build is the source of truth for the flags brain will emit.

---

## 1. `AgentKind`

`src/agent/session.rs` owns the enum. Add the variant, extend `ALL` (display
order is the order the UI and `brain env` prompts list), and add the stable
`as_str()` string. That string is persisted in the state DB and in
`default_agent_frontend`, so it is a compatibility surface: choose it once.

`AgentKind::label()` reads from the registry, so there is nothing else to add.

## 2. The adapter

One module under `src/agent/` (`src/agent/<name>.rs`, plus a directory of
submodules when it needs more than one file: keep every file under the
~400-line production budget). It implements `AgentFrontend`:

| Method | Contract |
| --- | --- |
| `kind` | the new `AgentKind` |
| `ensure_available` | run the compatibility probe; a known-but-unusable frontend must fail here, before any PTY spawn |
| `launch_spec` | the full launch: command, cwd (always the workspace root), explicit environment, hook metadata, and the honest capability report |
| `rollback_launch` | remove anything `launch_spec` wrote when the child failed to start |
| `input_for` | one atomic terminal sequence per `AgentAction` |
| `completion_strategy` | `Hook` when the frontend can report turn completion, `TransportExit` otherwise |
| `resume_candidate_exists` | read-only proof that this exact session is still resumable |
| `response_id` | stable artifact identity for a launched session |
| `can_resume_response_session` | whether a completed receiver session can be picked back up |

Two rules that are easy to get wrong:

- **Every interpolated value is shell-quoted** with `frontend::shell_quote`.
  The command string is handed to `sh -c`.
- **`command_for` is the static builder** the registry calls, and it only gets
  the configured command, the session plan, and the prompt. Anything that needs
  the workspace (a config path, an extension path, the boundary prompt) belongs
  in a richer private builder that `launch_spec` calls. Keep the two consistent:
  with no capability plan and no policy, they must produce the same string,
  because the adapter contract suite asserts exactly that.

## 3. The lifecycle bridge

Brain's frontend-neutral bridges are `scripts/agent_session_start_hook.py`
(session rotation), `scripts/agent_session_stop_hook.py` (completion
publication), and `scripts/receiver_observation_bridge.py` (content-free
receiver observations). A new frontend does **not** get new bridge logic: it
gets a thin adapter that turns that frontend's events into the bridges' JSON
payloads.

| Bridge | stdin payload |
| --- | --- |
| session start | `{"session_id": ..., "source": "startup" \| "new" \| "resume" \| "fork" \| ...}` (a `fork` source is ignored by the bridge) |
| session stop | `{"session_id": ..., "last_assistant_message": ...}` |
| observation, accept | `{"hook_event_name": "UserPromptSubmit", "session_id": ..., "turn_id": ..., "prompt": ...}` |
| observation, progress | `{"hook_event_name": "PostToolUse", "session_id": ..., "turn_id": ..., "tool_use_id": ...}` |

`accepted_turn_id` in `receiver_observation_bridge.py` maps each
`BRAIN_AGENT_KIND` to the payload field carrying the turn id. Add the new kind
there or every observation it writes is rejected.

The bridges read the `BRAIN_*` identity variables from the environment, so the
adapter must forward them (see `RUNTIME_ENVIRONMENT` / `BRAIN_ENVIRONMENT` in
`scripts/opencode_brain_plugin.js` and `scripts/pi_brain_extension.ts` for the
exact allowlist) and must never raise: a crash inside the agent is loud and
would distract from the session.

Declare how the artifact is installed in `src/agent/registry/contract.rs`:

- `LifecyclePayload::StaticFile` writes bundled source (a plugin, an extension)
  at a workspace-relative path with a fixed mode.
- `LifecyclePayload::HookSettings` merges brain's normalized hook commands into
  a frontend's JSON settings file, using `HookCommandStyle` to choose how the
  command names the workspace root.

Every registration's lifecycle installations are installed for every workspace,
whichever frontend is selected, so ids must be globally unique (there is a test)
and the three shared Python bridges are declared once (under Claude) rather than
repeated.

Then declare `HealthCheckDescriptor`s for the same artifacts: `FileContents`
requires the exact bundled source (so a stale copy fails on its own) and `Hook`
requires a configured event whose command ends with the current script suffix.
`brain tasks doctor` reports one row per check.

## 4. The compatibility probe

Model it on `src/agent/claude/probe.rs` (version floor) and
`src/agent/opencode/probe.rs` (capability interrogation). A probe:

- runs read-only commands through `command_probe::ShellProbeRunner`, which
  gives each probe its own process group and a null stdin so a probe can never
  be stopped by the controlling terminal while a TUI is open;
- uses `run_isolated` (a disposable `HOME` and XDG root, removed afterwards) for
  anything that could otherwise touch the user's configuration, and the real
  environment only for a check that is *about* the user's configuration, and
  then only with read-only flags;
- caches successful results per command string;
- returns `AgentError::Frontend` with a message that says what is wrong **and**
  the exact command to fix it (`brain env set <name>_cmd <command>`).

Wire it as the registration's `compatibility_probe` so `brain tasks doctor`
prints a row for it and `Diagnosis::is_ok` requires it. Leave it `None` only
when compatibility is genuinely declared by an installed artifact instead.

## 5. Register it

`src/agent/registry.rs`: one `FrontendRegistration` with the kind, label,
`<name>_cmd` command key, default command, constructor, `command_builder`,
lifecycle installations, health checks, capability evidence, and the optional
compatibility probe. Bump the array length. The registry is the only place that
knows the set of frontends; everything downstream iterates it.

## 6. Selection surfaces

Both halves, in the same change:

- **CLI selector** in `src/cli/global.rs`: a long flag and a short alias, in
  `normalize_global_args` (so it is accepted before *and* after a subcommand),
  in the `Cli` struct, in `selected_agent`, and in the conflict error message.
- **`default_agent_frontend`** in `src/agent/default_frontend.rs`: `parse` must
  accept the stable string (plus any hyphen or underscore spelling of it), and
  the `InvalidFrontend` message lists the valid set.
- **`<name>_cmd`** in `src/env/schema.rs`: one `VarSpec` naming the default
  command and what brain appends to it. Bump the `VARS` array length.

## 7. Persistence

The stable `as_str()` value is written to SQLite, and three tables constrain it:
`receiver_session_registrations`, `receiver_answer_cleanups`, and
`manual_sessions`. **None of that needs editing.** The allowed list is generated
from the frontend registry (`src/state/frontend_contract.rs`), any table whose
stored `CHECK` has drifted is rebuilt when the database is opened, and both
string parsers (`src/state/receiver/store/load.rs`,
`src/state/receiver/store/answer_cleanup.rs`) go through
`AgentKind::parse_exact`.

What a new frontend does owe here is a **startup migration** under
`src/startup_migration/` with `up` **and** `down`. `up` opens each workspace
state database, which is what runs the rebuild. `down` must leave a database the
*previous* Brain accepts: restore that version's contract and drop the rows only
the new frontend could own (`pi_frontend.rs` is the reference; a downgrade runs
from this binary, so it is this binary that has to know the older shape).

## 8. Capabilities

`AccessPolicy` resolves a `CapabilityPlan` (selected MCPs and skills) that is
frontend-independent; the adapter decides how much of it the launch can
actually enforce and reports that honestly through
`CapabilityPlan::enforcement_report(evidence)`:

- `StrictlySelected`: the launch arguments exclude every non-selected source.
- `AdvisoryOnly`: the selection is trusted guidance and other sources remain.
- `Unavailable`: the capability cannot be launched at all (no machine material,
  or the frontend has no such mechanism).

Never overstate. If the frontend has no MCP support, say so with
`EnforcementEvidence::without_mcp_support()` rather than reporting
advisory-only for a mechanism that does not exist.

## 9. Skills

Rendered brain skills live in `<workspace-root>/.agents/skills`.
`src/skills/layout.rs` additionally symlinks them into each project-local
frontend skills directory. Add the new frontend there **only** if it discovers
skills from its own project directory; a frontend that can be pointed at a
directory on the command line should be pointed at the canonical
`.agents/skills` instead, which needs no symlink and no project trust.

## 10. Tests

Red first, always. The places a new frontend must appear:

- `src/agent/adapter_tests/contract.rs`: add a row to `frontend_contracts()`.
  That one row runs the whole shared suite (launch commands for both plans with
  and without a prompt, quote-heavy prompt bounds, input sequences, completion
  strategy, receiver resume, controller integration).
- `src/agent/registry.rs` tests: the registry stays exhaustive and unique.
- The `for kind in AgentKind::ALL` loops in `src/tui/app_brain/tests/` and
  `src/tui/event_loop/setup/tests.rs` pick the new frontend up automatically.
  If one of them fails, that is a real parity gap, not a test to narrow.
- Adapter-specific unit tests for the pure decisions: command construction,
  session-file discovery, probe parsing, and the bridge payload mapping.

## 11. Docs and version

Per the docs contract in `AGENTS.md`, a new frontend touches
[architecture.md](architecture.md), [features.md](features.md),
[integrations.md](integrations.md), [config.md](config.md),
[data-model.md](data-model.md), [decisions.md](decisions.md),
[testing.md](testing.md), and this file's frontend list. Bump the crate version
(a new frontend is an additive user-visible feature, so the minor version) and
move `Cargo.lock` with it.

---

## File map

| Seam | File |
| --- | --- |
| Frontend identity | `src/agent/session.rs` |
| Adapter | `src/agent/<name>.rs` |
| Launch/input/session contract | `src/agent/frontend.rs` |
| Registry | `src/agent/registry.rs` |
| Lifecycle + health descriptors | `src/agent/registry/contract.rs` |
| Lifecycle installer | `src/command/server/receiver/hooks.rs` |
| Bridge scripts | `scripts/` |
| Probe runner | `src/agent/command_probe.rs` |
| Direct-invocation parsing | `src/agent/direct_command.rs` |
| Selector flag | `src/cli/global.rs` |
| Stored default | `src/agent/default_frontend.rs` |
| Command env var | `src/env/schema.rs` |
| State schemas | `src/state/receiver/schema*.rs`, `src/state/manual_session/schema.rs` |
| Startup migrations | `src/startup_migration/` |
| Capability enforcement | `src/access/enforcement.rs` |
| Skill fan-out | `src/skills/layout.rs` |
| Doctor | `src/tasks/doctor/` |
| Shared adapter contract suite | `src/agent/adapter_tests/contract.rs` |
