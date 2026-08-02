# Brain Multi-Workspace Design

**Date:** 2026-08-02
**Status:** Approved in design review

## Summary

Brain will support multiple fully separate workspaces on one computer through a
single binary. A workspace is one Brain root plus its portable data,
configuration, users, agent sessions, sync state, and capabilities.

The machine keeps one registry of its workspaces. Every command selects one
workspace explicitly with `--brain` or `-b`, or implicitly through the
machine's default selection. Selection never changes a workspace's access
mode.

Workspaces are trusted environments. `workspace_only` access uses prompts,
working-directory selection, and capability filtering to reduce accidental or
naive cross-workspace access. It is not a security boundary and must not be
described as one.

## Goals

- Let one Brain binary manage multiple roots on one computer.
- Preserve the current single-workspace experience for existing users.
- Permit different workspace TUIs to run concurrently.
- Prevent two TUIs from opening the same workspace concurrently.
- Keep portable data and machine-local environment values separate per
  workspace.
- Resolve interactive and inbound actions to a portable workspace user.
- Assign new tasks to the effective actor.
- Give every task an immutable merge identity without sacrificing `T###`
  display IDs.
- Make triage habits optional per workspace.
- Share one TUI-lifetime server process without sharing workspace agent state.
- Centralize Claude, Codex, and future OpenCode behavior behind an agent
  facade.
- Make every production change through observed red/green TDD.

## Non-goals

- Adversarial multi-tenancy.
- Filesystem sandboxing or containerization.
- Separate operating-system users.
- Separate Claude, Codex, or OpenCode provider accounts.
- Durable inbound-message queues.
- Headless agent execution while a workspace TUI is closed.
- Functional OpenCode sessions in this change.
- An audit history of who created or edited every task.

## Vocabulary

- **Workspace:** One Brain root and its portable state.
- **Canonical workspace name:** The machine registry key used by `--brain`.
- **Workspace alias:** A machine-local alternate selector for a canonical name.
- **Workspace UUID:** Stable internal identity used for caches, locks, sync, and
  receiver routing.
- **Default workspace:** The workspace selected when `--brain` is omitted.
- **Local user:** The portable workspace user selected by this machine's
  `local_user_id`.
- **Effective actor:** The user attributed to the current request.
- **Unrestricted access:** The normal trusted personal Brain behavior.
- **Workspace-only access:** Advisory root instructions and capability
  filtering, without a filesystem security boundary.
- **Shared server:** The machine-wide, TUI-lifetime process hosting local web
  and receiver routes.

## Workspace registry

The machine registry remains one file at `~/.config/brain/env.json`. It is
keyed by canonical workspace name:

```json
{
  "schema_version": 2,
  "default_workspace": "brain",
  "workspaces": {
    "brain": {
      "workspace_id": "stable-uuid",
      "root": "~/brain",
      "aliases": [],
      "local_user_id": "pablo",
      "receiver_enabled": false,
      "env": {}
    },
    "family": {
      "workspace_id": "stable-uuid",
      "root": "~/family",
      "aliases": ["fam"],
      "local_user_id": "pablo",
      "receiver_enabled": true,
      "env": {}
    }
  }
}
```

Top-level state is limited to registry schema and default selection. Workspace
records never inherit environment values from one another.

Machine-local workspace state includes:

- Root path.
- Aliases.
- Local-user selection.
- Receiver enablement.
- Agent frontend commands.
- Provider secrets.
- MCP credentials and connection details.
- Sync transport configuration.
- Other machine-specific paths and environment values.

The root basename supplies the initial canonical name. Names and aliases are
unique under the same case-folding rules used for lookup. Brain rejects
ambiguous aliases, duplicate UUID registrations, and overlapping roots.

Changing the default workspace changes only command selection. Access mode is
owned by the workspace and remains unchanged.

Renaming atomically rekeys the registry record while retaining the workspace
UUID, caches, sync identity, and receiver ingress identity.

## Portable workspace state

Each root contains:

```text
<root>/.config/
├── workspace.json
├── config.json
├── users.json
├── personalization.json
├── extensions/
└── plugins/
```

`workspace.json` contains the stable workspace UUID, portable data schema
version, receiver ingress ID, and compatible Brain version information.

`config.json` contains portable behavior, including:

- `access_mode`
- `enable_triage_habits`
- MCP allowlist
- Skill allowlist
- Existing portable Brain settings

`users.json` contains portable users and inbound sender mappings.

## Workspace runtime state

Runtime state is derived from the workspace UUID:

```text
~/.cache/brain/workspaces/<workspace-uuid>/
├── state.db
├── tui.lock
├── inbox/
├── responses/
├── logs/
└── sync/
```

Session state, attachments, response files, logs, sync baselines, journals,
watchers, and locks cannot share paths across workspaces.

The shared server may keep a machine-global socket, PID record, and routing
log under `~/.cache/brain/server/`. These contain infrastructure state only.
The shared log must not contain message bodies, credentials, or task data.
Workspace-specific detail belongs in that workspace's cache.

## Workspace resolution

Every user command follows this order:

1. Parse `--brain` or `-b` as a global option.
2. Resolve a canonical name or alias.
3. Fall back to `default_workspace`.
4. Validate the root and its portable workspace UUID.
5. Construct an immutable `WorkspaceContext`.
6. Run readiness and migration checks.
7. Dispatch the command with that context.

Brain does not expose a process-global mutable root. `WorkspaceContext` is
passed explicitly to commands, TUI state, sync, skills, servers, and agent
controllers.

Child processes and skill scripts receive only the integration variables they
need:

```text
BRAIN_WORKSPACE_ID
BRAIN_WORKSPACE
BRAIN_ROOT
BRAIN_ACTOR_ID
```

These variables are an integration boundary, not Brain's internal source of
truth. Detached Brain commands also carry the canonical workspace selector
explicitly.

## CLI surface

The workspace selector works before or after subcommands:

```sh
brain -b family
brain sync -b family
brain config get access-mode --brain fam
```

Workspace management commands are:

```sh
brain workspace list
brain workspace create [--name <name>] [--root <path>]
brain workspace attach <root>
brain workspace rename <workspace> <name>
brain workspace alias add <workspace> <alias>
brain workspace alias remove <workspace> <alias>
brain workspace default <workspace>
brain workspace remove <workspace>

brain user list
brain user add --id <id> --name <name>
brain user update <id>
brain user remove <id> [--reassign-to <id>]
brain user local <id>
```

Omitted values open themed interactive setup. All actions also have complete
non-interactive flags.

`attach` validates the portable manifest before registering an existing root.
Removing a registry entry never deletes the root, portable data, or remote
data.

`workspace list` shows canonical name, default state, root, access mode, local
user, receiver state, and aliases.

User commands operate on the selected workspace. `user local` changes only the
machine registry's `local_user_id`; the other user mutations change the
portable registry. Removing a user with assigned tasks requires an explicit
replacement user.

`brain env` edits the selected workspace's machine-local registry section.
`brain config` edits the selected workspace's portable configuration.

## Readiness and migration gate

Every interactive user command checks the selected workspace before operating
on its data. Missing required values launch guided setup, after which the
original command continues.

Non-interactive commands never wait for input. They fail with exact commands
for supplying each missing value.

The following do not prompt:

- `--help`
- `--version`
- Agent hooks
- Internal shared-server invocations

Internal operations treat an incomplete workspace as unavailable.

The existing installation migrates into the first workspace:

- Canonical name comes from the root basename, normally `brain`.
- It becomes the default workspace.
- Its access mode becomes `unrestricted`.
- Existing environment and portable configuration are preserved.
- Existing personalization name becomes the first portable user.
- That user becomes the machine's `local_user_id`.
- Existing allowed senders and response email are mapped interactively when
  they cannot be assigned unambiguously.

The first new installation also defaults its first workspace to
`unrestricted`. Later workspaces default to `workspace_only`. Setup permits an
explicit override.

## Portable users and actors

There is no owner identity. Users are portable workspace members whose IDs are
unique within that workspace:

```json
{
  "users": [
    {
      "id": "pablo",
      "name": "Pablo",
      "phones": [
        {
          "value": "+1...",
          "inbound_allowed": true
        }
      ],
      "emails": [
        {
          "value": "pablo@example.com",
          "inbound_allowed": true
        }
      ],
      "response_email": "pablo@example.com"
    }
  ]
}
```

Phone numbers are normalized to E.164. Emails use one documented
normalization rule. An enabled phone number or email cannot identify two users
in the same workspace.

An optional `response_email` must name one normalized email on the same user.
It is the only unrelated-thread delivery target for that user; authenticated
inbound email replies may also target an allowlisted participant already in the
thread under the existing delivery rules.

Each machine selects one registry member through `local_user_id`. The same ID
may be selected on many computers. For example, both `mbpro` and
`avandarmini` can select `pablo`.

Every request receives an `ActorContext`:

1. An authenticated inbound SMS or email uses the user mapped from its sender.
2. Otherwise, interactive terminal and TUI requests use `local_user_id`.
3. Agent follow-up work inherits the initiating actor.
4. Unknown or disallowed inbound senders are rejected.

Inbound identity overrides local identity. The workspace, actor, and channel
are fixed before the prompt reaches the agent.

## Tasks and stable identity

Tasks gain:

- An immutable internal `task_uuid` used for merge identity.
- The existing human-facing `T###` display ID.
- `assigned_to`, containing a workspace user ID.

New tasks default `assigned_to` to the effective actor. Editing a task does not
change assignment unless reassignment is explicit. No creator or audit column
is added.

Readers temporarily accept the legacy `assignee` name. Writers emit
`assigned_to` after migration.

Before the schema migration, Brain completes a legacy semantic sync. Existing
tasks then receive deterministic UUIDs derived from workspace UUID plus legacy
task ID. Newly created tasks receive generated immutable UUIDs.

When independent machines create the same display ID, semantic merge keeps
both UUID-distinct tasks. A pure reconciliation step deterministically
renumbers one display ID and updates task relationships. Repeated syncs produce
the same result.

CSV merge aligns fields by column name rather than column position. It
preserves supported columns and refuses unsupported schema versions instead of
guessing.

## Triage habits

The portable boolean `enable_triage_habits` defaults to `true`.

When enabled:

- Brain reconciles one managed daily definition and one managed weekly
  definition.
- Stable system markers identify them independently of their visible names.
- Users cannot delete the definitions or managed chains.
- Sync repair restores required managed definitions.
- The existing daily-modal preference continues to control whether the modal
  appears.

When changed to `false`, Brain purges:

- Managed daily and weekly definitions.
- Open generated triage tasks.
- Completed triage task history.
- Corresponding derived index entries.

The daily modal is suppressed regardless of its separate preference. Manual
use of the triage skill remains available. Re-enabling creates fresh managed
definitions without restoring history.

The purge affects triage habit and task records, not unrelated agent
transcripts.

## Advisory workspace access

Access mode is portable and workspace-owned:

- `unrestricted`
- `workspace_only`

`workspace_only` is advisory. It is not a filesystem sandbox or an adversarial
security boundary.

Brain applies these guardrails:

- Launch the agent with the workspace root as its working directory.
- Inject a prominent system instruction naming the canonical root.
- Instruct the agent not to read, write, inspect, reveal, or execute against
  paths outside the root.
- Instruct the agent to reject requests that conflict with that boundary.
- Avoid passing credentials belonging to other workspaces.
- Load only allowed MCPs and skills where the frontend can select them.
- Avoid adding unrelated directories and tools.
- Apply the same instructions and capability selection to interactive and
  inbound prompts.

Inbound prompts cannot change access mode. Only trusted Brain configuration
commands may do so.

The README, setup flow, configuration docs, and status output state plainly:

- Workspace-only access is prompt-enforced.
- It reduces mistakes and naive leakage.
- A determined user or prompt injection can bypass it.
- It is unsuitable for adversarial users or sensitive tenant isolation.
- Real isolation requires separate operating-system accounts, machines, VMs,
  or containers outside Brain.

Suggested status output is:

```text
Access mode  workspace-only
Enforcement  advisory prompts and capability filtering
Sandbox      none
```

## MCP and skill selection

The portable workspace config stores logical MCP and skill allowlists.
Machine-local environment records store credentials, executable paths, and
connection details.

An unrestricted workspace may continue to use the user's normal global agent
configuration. A workspace-only launch exposes only its allowlisted MCPs and
skills where the frontend supports strict selection. When exclusion cannot be
enforced, Brain supplies an explicit prompt restriction and documents the
limitation.

New workspace-only workspaces begin with a small Brain core skill set:

- `todo`
- `second-brain`
- `contacts`
- `triage`

Additional skills are enabled explicitly. Bundled skills must be generic and
resolve their root and actor from the launch context. They must not contain
personal workspace data or hard-coded `~/brain` paths.

## AgentController facade

Frontend behavior moves behind this module hierarchy:

```text
src/agent/
├── mod.rs
├── controller.rs
├── frontend.rs
├── claude.rs
├── codex.rs
├── opencode.rs
├── input.rs
├── hooks.rs
└── session.rs
```

`AgentController` receives `WorkspaceContext`, `ActorContext`, and the selected
frontend. Its interface uses semantic operations:

- Build a launch command.
- Start or resume a session.
- Type text.
- Submit immediately.
- Queue after an active turn.
- Observe session start and completion.
- Resolve transcript and session identity.
- Shut down cleanly.

Claude and Codex controllers translate these operations into frontend-specific
commands, hooks, PTY input, submission keys, queueing, and resume behavior.
TUI surfaces and receiver dispatch do not branch directly on agent kind.

Session persistence becomes frontend-neutral and workspace-local. It stores
agent kind, agent session ID, channel, actor context, and completion state.

### OpenCode stub

This change:

- Adds `AgentKind::OpenCode`.
- Adds `opencode_cmd`.
- Recognizes `--open-code` and `-oc`.
- Constructs the controller through the facade.
- Fails before TUI launch with a themed not-implemented error.
- Adds parsing, selection, construction, and unsupported-launch smoke tests.

It does not implement OpenCode prompt delivery, hooks, completion, resume, or
PTY behavior.

Passing `--codex` and `--open-code` together produces a themed red error with
an emoji and instruction to choose one frontend.

## Shared server and receiver

One machine-wide process hosts local web routes and inbound receiver routes.
It is shared infrastructure, not shared agent state.

Lifecycle:

1. The first TUI starts or connects to the shared process.
2. Every TUI registers a workspace lease and sends heartbeats.
3. Different workspaces may have live leases simultaneously.
4. The per-workspace TUI lock prevents two TUIs for the same workspace.
5. Closing a TUI removes its lease.
6. The final TUI closing terminates the shared process.
7. If the process crashes while TUIs remain open, a TUI re-elects a starter
   and restores live registrations.

There is no server, receiver, availability responder, or background Brain
process when every TUI is closed.

Receiver enablement is persistent, machine-local, and workspace-specific:

- Enabled plus a live TUI lease accepts messages.
- Disabled or missing TUI lease, while another TUI keeps the process alive,
  sends a concise unavailable response and discards the message.
- No shared process produces no Brain response.
- Accepted work may use the existing TUI-owned in-memory turn queue.
- There is no durable queue, replay, or headless agent execution.

`--with-receiver` persistently enables the selected workspace. The command
palette and `brain receiver start|stop` update the same setting. An enabled
workspace registers automatically whenever its TUI opens.

Every workspace has a stable opaque receiver ingress ID. SMS and email paths
identify the workspace before provider authentication and sender-to-user
mapping. Public base URLs and provider credentials remain machine-local.

The command surface is:

```text
brain server status
brain server logs

brain receiver setup
brain receiver set
brain receiver start
brain receiver stop
brain receiver status
brain receiver logs
```

Manual server start, kill, and all restart commands are removed. Configuration
reloads live.

## Sync and workspace joining

Every sync artifact is workspace-specific:

- Lock
- Journal
- Current-run state
- Bisync work directory
- CSV baselines
- Watch and debounce state
- Receiver freshness state

Different workspaces may sync concurrently. The same workspace remains
serialized. Startup, watcher, and receiver triggers carry the workspace
identity explicitly.

Portable data syncs with the workspace. Machine-local registry fields and
secrets do not.

Attaching or joining a workspace validates its portable UUID. Brain refuses to
register a root whose UUID conflicts with another local registration. Sync
setup warns when a remote target is already associated with a different
workspace UUID.

The portable data schema records its minimum compatible Brain version. A
newer schema causes older supported clients to refuse the workspace rather
than write it. Before migration, setup tells the user to update all computers
that sync the workspace and creates backups of the legacy CSVs.

## Error handling

Expected failures use typed errors and themed terminal output:

- Unknown or ambiguous workspace.
- Duplicate canonical name or alias.
- Overlapping root.
- Workspace UUID mismatch.
- Missing local user.
- Unknown or disallowed inbound sender.
- Receiver disabled.
- Workspace TUI unavailable.
- Unsupported agent frontend.
- Unsupported workspace schema.
- Advisory cross-workspace request rejection.

Long-running commands narrate each meaningful phase with normal themed output,
not only under `--verbose`.

## Red/green TDD

Every production behavior follows the repository's iron law:

1. Write the smallest failing test.
2. Run it and observe the expected failure.
3. Write only enough production code to pass.
4. Refactor while green.
5. Repeat for the next behavior.

Behavior-preserving extractions begin with characterization tests. No
production facade, migration, schema, lifecycle, or CLI behavior is written
before its failing test.

Required coverage includes:

- Registry parsing, migration, defaults, aliases, renames, and validation.
- Workspace selection before and after subcommands.
- Workspace-derived path non-collision.
- Per-workspace TUI and sync lock behavior.
- Readiness prompting and non-interactive errors.
- User normalization and actor precedence.
- Assignment defaults and edit preservation.
- UUID migration and duplicate `T###` reconciliation.
- Relationship rewrites and idempotent repeated syncs.
- Managed triage protection and complete disable purge.
- Access-mode prompt construction and capability selection.
- Agent facade characterization and OpenCode smoke behavior.
- Shared-server election, leases, crash recovery, and final shutdown.
- Persistent receiver toggles across CLI and palette.
- Receiver routing, unavailability, and no-queue behavior.
- Workspace-specific sync state and schema gates.

## Acceptance scenario

The finished system proves this scenario:

1. `mbpro` runs the unrestricted `brain` workspace at `~/brain`.
2. `avandarmini` runs the same synced `brain` workspace with local user
   `pablo`.
3. `avandarmini` also runs a workspace-only `family` workspace at `~/family`.
4. The `family` workspace has portable users `pablo` and the user's wife.
5. The wife's computer attaches the synced `family` workspace and selects her
   user ID as `local_user_id`.
6. Personal and family TUIs run simultaneously on `avandarmini`.
7. Each TUI has separate sessions, caches, tasks, sync state, and agent
   context.
8. An inbound family SMS maps to the wife's user and assigns new tasks to her.
9. Closing only the family TUI makes family inbound routes unavailable while
   the personal TUI keeps the process alive.
10. Closing the final TUI terminates the shared server.
11. Family triage is disabled and all managed triage history is absent.
12. A naive request to inspect `~/brain` is rejected by the family agent's
    advisory instructions.
13. Documentation makes clear that a determined request may bypass those
    instructions.

## Documentation and release requirements

Implementation updates all relevant durable documentation in the same change:

- `README.md`
- `docs/glossary.md`
- `docs/architecture.md`
- `docs/features.md`
- `docs/data-model.md`
- `docs/config.md`
- `docs/integrations.md`
- `docs/decisions.md`
- `docs/testing.md`
- `docs/keybindings.md`
- Skill documentation affected by workspace context

The implementation also updates every CLI help surface, command-palette row,
shortcut annotation, and setup prompt affected by receiver or frontend changes.

Before completion, run the full release test suite, Clippy across all targets,
the personal-data guard, migration fixtures, and the end-to-end acceptance
scenario. This additive pre-1.0 feature receives a minor version bump. Every
committed implementation change bumps the crate version according to project
policy.
