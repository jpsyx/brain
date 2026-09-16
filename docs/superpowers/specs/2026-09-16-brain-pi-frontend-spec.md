# Brain pi frontend design spec

**Goal:** make **pi** (`@earendil-works/pi-coding-agent`) a fully supported
Brain agent frontend, with the same facade-level launch, input, session,
completion, receiver, skill-session, and shutdown contract as Claude, Codex, and
OpenCode, while preserving the user's own pi configuration and reporting
unsupported capabilities honestly.

**Baseline:** Brain `0.90.1`, branch `feat/pi-frontend`, locally installed pi
`0.85.1` at `~/.nvm/versions/node/v24.19.0/bin/pi`.

**Method:** the contract in [../../adding-an-agent-frontend.md](../../adding-an-agent-frontend.md).

---

## 1. Verified pi facts (pi 0.85.1, 2026-09-16)

Verified against the installed binary (`pi --help`, `pi --version`,
`pi --list-models`, `pi auth check`) and the docs bundled with that build
(`.../pi-coding-agent/docs/*.md`), not only the website.

### Launch and sessions

- `--session-id <id>` uses an **exact project session id, creating it if
  missing**. It cannot be combined with `--session`, `--continue`, or
  `--resume`. Ids must match `^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$`, so
  a Brain UUID is valid. Introduced in pi 0.76.0; since 0.80.4 a create prints a
  warning line to stderr.
- Because `--session-id` opens an existing session and creates a missing one,
  **one flag serves both `SessionPlan::Fresh` and `SessionPlan::Resume`**, and
  Brain's chosen id stays authoritative exactly as it is for Claude.
- Sessions are JSONL files at
  `<agent-dir>/sessions/--<cwd with the leading separator dropped and `/`, `\`,
  `:` replaced by `-`>--/<ISO timestamp with `:` and `.` replaced by `-`>_<session-id>.jsonl`.
  `<agent-dir>` is `$PI_CODING_AGENT_DIR` or `~/.pi/agent`.
  `$PI_CODING_AGENT_SESSION_DIR` (or `--session-dir`) replaces the whole
  directory with a flat one.
- The file is written lazily: a session that never took a turn leaves no file,
  so file existence is exactly the right resume evidence.
- An initial prompt is passed after `--`, which stops option parsing, and leaves
  pi interactive.
- `--append-system-prompt <text>` appends to the system prompt.

### Extensions and lifecycle events

- Extensions are TypeScript modules loaded through jiti, exporting a default
  factory `(pi: ExtensionAPI) => void`.
- `-e/--extension <path>` loads one explicitly. **CLI extensions are loaded
  before trust resolution and bypass the project-trust prompt**, and they load
  even with `--no-extensions`.
- Events Brain needs, all confirmed present in 0.85.1:
  `session_start` (`event.reason` is `startup | reload | new | resume | fork`),
  `message_end` (finalized message, `role`, `content` blocks, `stopReason`),
  `tool_execution_end` (`toolCallId`, `toolName`, `isError`),
  `input` (`event.text`, `event.source`), and `agent_settled`, which fires when
  pi will not continue automatically (retry, compaction, or queued follow-up all
  settled). `agent_settled` was added in pi 0.80.4.
- `ctx.sessionManager.getSessionId()` returns the current session id inside any
  handler, including `session_start`.
- pi runs on Node, so a bridge spawns `python3` with `node:child_process`.

### Project trust

- `.pi/settings.json`, `.pi/` resources, and a project `.agents/skills`
  directory require project trust. A Brain workspace root always contains
  `.agents/skills`, so an untrusted first launch **would prompt inside the
  panel**.
- `--no-approve` declines project-local trust for one run without storing a
  decision. Explicit `-e` extensions and explicit `--skill` paths still load.

### Skills, MCP, capabilities

- `--skill <path>` is repeatable, accepts a directory of skills, and is
  **additive even with `--no-skills`**; CLI skill paths are never trust-gated.
- `--no-skills` disables discovery of every other skill location.
- pi has **no built-in MCP** ("It intentionally does not include built-in MCP,
  sub-agents, permission popups, plan mode, to-dos, or background bash").

### Input

- `tui.input.submit` is `enter`.
- While a turn is running, **Enter queues a steering message**, injected into
  the running turn once it finishes its tool calls, and **`alt+enter`
  (`app.message.followUp`) queues a follow-up** delivered after the agent
  finishes all work. `handleFollowUp` falls back to an ordinary submit when pi
  is not streaming, so the follow-up key is also correct for an idle pi.
- pi's key parser accepts `alt+enter` as the kitty CSI u sequence
  `ESC [ 13 ; 3 u` whether or not its kitty keyboard protocol is active, while
  the legacy `ESC CR` encoding is read as **shift+enter** (a newline) once that
  protocol is on.
- `/new` starts a new session.

### Configuration and readiness

- `pi --list-models` prints a `provider  model  …` table of the models whose
  provider credentials actually resolve, and prints `No models available.` when
  none do. It is therefore the readiness check for "this machine's pi can
  actually run a turn".
- `PI_OFFLINE=1` disables startup network operations, which keeps the check
  fast and offline-safe.
- pi's own process configuration lives in the `PI_*` namespace
  (`PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR`, `PI_OFFLINE`, …). A
  separate set of `PI_*` variables (`PI_SESSION_ID`, `PI_SESSION_FILE`,
  `PI_PROVIDER`, `PI_MODEL`, `PI_REASONING_LEVEL`, `PI_CODING_AGENT`) is
  *per-session metadata pi injects into its own shell tools* and must never be
  inherited by a new pi process.
- First-time setup only runs with `PI_EXPERIMENTAL=1`, so it cannot block a
  Brain panel.

---

## 2. Required behavior

### 2.1 Selection

| Surface | Value |
| --- | --- |
| `AgentKind` | `Pi`, stable string `pi`, listed last in `ALL` |
| CLI flag | `--pi`, alias `-pi`, accepted before and after a subcommand, conflicting with the other selectors |
| Stored default | `brain env set default_agent_frontend=pi` |
| Launch command | `brain env set pi_cmd <command>`, default `pi` |

### 2.2 Launch

```
cd <workspace-root> && <pi_cmd> --no-approve [--no-skills] [--skill <dir>]
    [--append-system-prompt <boundary prompt>] [-e <root>/.brain/hooks/pi_brain_extension.ts]
    --session-id <brain session id> [-- <initial prompt>]
```

- `--no-approve` is unconditional: it keeps the panel from ever stopping on a
  trust prompt, and makes every machine's launch identical. The user's *global*
  pi configuration (`~/.pi/agent/...`, global skills, global extensions) is
  untouched; only project-local `.pi` resources inside the Brain root are
  ignored, and Brain passes what it needs explicitly.
- The lifecycle extension is passed with `-e` rather than installed into
  `.pi/extensions/`, so it loads without trust and cannot be double-loaded.
- Skills: unrestricted access mode passes `--skill <root>/.agents/skills`;
  a restricted plan passes `--no-skills --skill <capability skills dir>`, which
  makes the selection exact.
- Environment: the shared minimal launch environment, plus the ambient `PI_*`
  configuration namespace **minus** pi's per-session metadata variables.

### 2.3 Input

| `AgentAction` | Bytes |
| --- | --- |
| `TypeText` | bracketed paste |
| `SubmitNow` | `\r` |
| `FollowUpAfterActiveTurn` | bracketed paste, settle, `ESC [ 13 ; 3 u` (Alt+Enter, pi's follow-up queue) |
| `StartNewSession` | `/new\r` |

### 2.4 Lifecycle

`scripts/pi_brain_extension.ts`, installed at
`<root>/.brain/hooks/pi_brain_extension.ts` (mode `0644`) and passed with `-e`:

| pi event | Brain bridge |
| --- | --- |
| `session_start` | session-start bridge, `{session_id, source: event.reason}` |
| `agent_settled` | session-stop bridge, `{session_id, last_assistant_message}` from the last non-error assistant message of the turn, published at most once per turn |
| `input` carrying the receiver job-token marker | observation bridge, `UserPromptSubmit`, with an extension-generated `turn_id` |
| `tool_execution_end` after an accepted turn | observation bridge, `PostToolUse`, `tool_use_id = toolCallId` |

`receiver_observation_bridge.py` learns that `pi` carries its turn id in
`turn_id` (like Codex and OpenCode).

### 2.5 Resume

`resume_candidate_exists` and `can_resume_response_session` both scan the
derived session directory for a file named `*_<session-id>.jsonl`. No file means
no resume candidate; Brain never guesses.

### 2.6 Availability and configuration inspection

`ensure_available` (before every launch) and `brain tasks doctor` share one
probe, which fails with an actionable message when:

| Check | Failure message names |
| --- | --- |
| the configured command runs | "pi is unavailable" + install / `brain env set pi_cmd` |
| `pi --version` parses and is >= the floor | the floor and how to update |
| `pi --help` advertises `--session-id`, `--append-system-prompt`, `--extension`, `--skill`, `--no-skills`, `--no-approve` | the missing flag |
| `pi --list-models` lists at least one model | that no provider credentials resolve, and that `/login` or a provider key is needed |

The first three run with a disposable `HOME`; the model check runs with the real
environment plus `PI_OFFLINE=1` and no writes, because it is a question *about*
the user's configuration.

**Version floor: 0.84.1.** That is the lowest release with every surface Brain
depends on (`--session-id` 0.76.0, `agent_settled` 0.80.4, the `--session-id`
create warning 0.80.4, and the auth-readiness surfaces 0.84.1).

### 2.7 Capabilities

| Capability | pi enforcement |
| --- | --- |
| Skills | `StrictlySelected` when Brain launches a direct `pi` invocation (the only case where `--no-skills --skill` is provably applied), else `AdvisoryOnly` |
| MCP | `Unavailable`: pi has no MCP mechanism at all |

### 2.8 Persistence

`pi` joins the `agent_kind` CHECK lists of `receiver_session_registrations`,
`receiver_answer_cleanups`, and `manual_sessions`, with a startup migration
(`up` and `down`) that rebuilds an already-created table whose stored SQL still
carries the old list.

---

## 3. Non-goals

- Do not modify the user's global pi configuration, credentials, settings,
  models, or trust store. Brain installs only its own workspace-scoped bridge.
- Do not call a paid or remote model from the test suite.
- Do not claim MCP, skill, or prompt filtering is a security sandbox.
- Do not add `.pi/skills` symlinks: pi is pointed at `.agents/skills` directly.
- Do not use pi's RPC/JSON modes or SDK in this change: the brain panel is a PTY
  running pi's own TUI, exactly as for the other three frontends.
