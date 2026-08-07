# Glossary

Plain-English terms the user says, mapped to the Rust identifiers / modules
they name. When the user refers to something by an everyday word, look it up
here to find the code. Keep this in sync when you rename a concept.

## Workspace vocabulary

| Plain English | What it means | Code / storage |
| --- | --- | --- |
| **workspace** / **brain workspace** | One selected Brain root plus its portable notes, tasks, config, personalization, manifest, and skill customizations. | `workspace::WorkspaceContext`; `<workspace-root>/` |
| **canonical workspace name** | The normalized registry key shown by `brain workspace list` and propagated to detached Brain children. It is trimmed, lower-case, and matches `[a-z0-9][a-z0-9_-]*`. | `workspace::WorkspaceName`; `MachineRegistry.workspaces` key |
| **workspace alias** | A machine-local alternate selector that resolves to one canonical name. | `WorkspaceRecord.aliases` |
| **workspace UUID** | Portable, immutable identity. It survives canonical rename, root changes across machines, aliases, and default changes, and keys runtime paths. | `WorkspaceId`; `.config/workspace.json`; `WorkspaceRecord.workspace_id` |
| **default workspace** | The canonical record selected only when `--brain/-b` is omitted. Changing the default workspace never changes access mode or any record field. | `MachineRegistry.default_workspace` |
| **selected workspace** | The one immutable root/name/UUID/local-user snapshot resolved at command bootstrap. Ordinary runtime code receives this context instead of reopening the registry. | `CommandContext`; `Arc<WorkspaceContext>` |
| **local user** / **local actor** | The immutable local `ActorContext` resolved once during ordinary-command bootstrap from the machine's `local_user_id`. A legacy-ready workspace without `users.json` retains that ID as an interactive compatibility actor without creating portable state. | `CommandContext::actor`; `actor::local_actor` |
| **workspace-only access** | Advisory prompt enforcement plus best-effort capability filtering. Brain supplies trusted boundary instructions, selected-root cwd, a minimal environment, and capability filtering. It is easy to bypass and is not tenant isolation. | `access::AccessMode::WorkspaceOnly`; `AccessPolicy`; `CapabilityPlan` |
| **unrestricted access** | The compatibility/default mode for the first workspace. Brain supplies no boundary prompt and lets the selected frontend use its ordinary global capabilities. | `access::AccessMode::Unrestricted` |
| **remote workspace identity** | The strict portable manifest at the configured sync target. Its UUID and schema must match the selected workspace before any remote data lane runs; setup may publish it only to an empty remote or after exact UUID adoption. | `sync::identity::VerifiedRemote`; remote `.config/workspace.json` |
| **migration journal** | UUID-scoped, machine-local progress for explicit legacy-to-multi-workspace cutover. It persists the original plan and backup across retries, then disappears after final verification. | `migration::MigrationJournal`; `<workspace-cache>/migrations/multi-workspace-v1.json` |
| **workspace requirement** | One centralized readiness/feature-health row. Availability is required; optional features are independently `off`, `ready`, or `incomplete`, never inherited from another workspace. | `workspace::requirements::{Requirement, RequirementsReport}` |

## The two-axis layout model

The merged `brain` shell has **three main views** and **one app-level panel**.
At most one main view shows at a time; the brain panel can be open alongside
it (split) or closed (main view full-width).

| Plain English | What it means | Code |
| --- | --- | --- |
| **main view** | One of the three full-screen surfaces you switch between. | `main_view::MainView` (`src/main_view.rs`) |
| **tasks view** | The task-management surface (agenda, triage, habits). The **default** at startup. | `MainView::Tasks`; state in the tasks App fields / `src/tasks/` modules |
| **brain directory view** / **brain search view** | The fuzzy-search-over-the-selected-workspace surface (this was *bare `brain`* before the merge). | `MainView::BrainSearch`; `src/picker/`, `src/entry.rs` |
| **logs view** | The scrollable diagnostic-log surface reached through the palette or the three-view cycle. | `MainView::Logs`; `src/tui/render/logs.rs` |
| **brain panel** | The live agent chat session in a PTY. Claude is the default; `--codex` / `-cx` selects Codex and `--open-code` / `-oc` selects OpenCode. App-level: the panel does **not** belong to a main view and stays open across a view switch. (Formerly called the *claude panel* in the `tasks` project.) | `src/agent/controller/` (`AgentController`); `src/pty_pane.rs` (`PtyPane` transport); `App.brain: Option<AgentController>`; `App.agent_kind` |
| **brain-panel tab** | Which session the brain panel is showing: the main session (`BrainTab::Main`) or the ephemeral **daily-triage** session (`BrainTab::Triage`), selected with `Alt+1` / `Alt+2`. The triage tab exists only while a daily-triage pass is running and is never tracked in the session DB. | `App.triage_brain: Option<AgentController>`; `App.active_brain_tab: BrainTab`; `src/tui/app_triage_tab.rs` |
| **panel** | Generic term; in this app the only panel is the brain panel. | — |
| **sub-view** | One of the tabbed modes *inside* the tasks view (`today`, `mit`, `past_due`, `week`, `habits`, `backlog`, `all`). These were called "views" in the old `tasks` project. `Tab` / `Shift+Tab` cycle them; only meaningful in the tasks view. | `view::View` + `View::CYCLE` (`src/tasks/view/`) |
| **focus** | Which surface receives keystrokes: the active main view, or the brain panel. `Alt+H`/`Alt+L` move focus between the spatial left/right halves. | `App.focus` |

## View switching vs. panel focus (two different axes)

These are deliberately distinct and use different modifiers:

| Plain English | Effect | Keys | Code |
| --- | --- | --- | --- |
| **cycle main views** | Change *which main view* is shown (tasks ↔ brain directory ↔ logs). | `Ctrl+H` (left) / `Ctrl+L` (right) | `main_view::ctrl_cycles_view` → `MainView::step` |
| **jump to a main view** | Go straight to one named main view. | `Ctrl+T` (tasks) / `Ctrl+B` (brain directory) | `main_view::ctrl_jumps_view` |
| **focus a panel** | Move keyboard focus between the main view and the brain panel (spatial left/right). | `Alt+H` / `Alt+L` | `App::focus_left` / `focus_right` |

## Modals and overlays

| Plain English | What it is | Code |
| --- | --- | --- |
| **command palette** | The filterable list of every command, opened with `Ctrl+P`. | `tui::palette` / `PaletteState` |
| **task actions modal** | The per-task command list opened with `Enter` on a task. | `PaletteState::new_task_actions` |
| **shortcuts modal** / **help** | The `Alt+S` keyboard-shortcuts reference. (Was bare `?` in `tasks`.) | `shortcuts::ALL`, `tui::draw_help` |
| **status line `?` hint** | The dim `Alt+S  all shortcuts` pointer at the end of the compact footer. | `shortcuts::footer_subset`, footer renderer |
| **confirm modal** | The Yes/No (or Yes/No/Skip) overlay for destructive or expensive actions. | `confirm` / `ConfirmState` |
| **brain-input modal** | The multi-line compose box that seeds a message into the brain panel. | `BrainInputState` |
| **link picker** | The numbered list of a task's openable links (`Ctrl+O`). | `LinkPickerState` |

## Infrastructure shared by both views

| Plain English | What it is | Code |
| --- | --- | --- |
| **agent frontend** | Which CLI adapter backs the brain panel: Claude by default, Codex with `--codex` / `-cx`, or OpenCode with `--open-code` / `-oc`. | `agent::AgentKind`; `Cli::selected_agent` |
| **agent controller** / **agent facade** | The frontend-neutral owner of one live agent. TUI and receiver callers request semantic launch, input, lifecycle, completion, terminal, and shutdown operations without building frontend commands or keystrokes. | `agent::AgentController`; `agent::AgentFrontend`; `agent::AgentTransport` |
| **frontend registry** | The exhaustive metadata table for functional frontends: identity, configured command key/default, constructor, command builder, lifecycle installations, health checks, capability evidence, and any compatibility probe. Shared callers iterate it instead of adding Claude/Codex/OpenCode branches. | `src/agent/registry.rs`; `src/agent/registry/contract.rs` |
| **the launch command** / **`claude_cmd` / `codex_cmd` / `opencode_cmd`** | The machine-local configured base command for a frontend. Its adapter adds frontend arguments, while the transport applies the selected workspace as cwd. | `agent::configured_command`; `ClaudeFrontend`; `CodexFrontend`; `OpenCodeFrontend` |
| **capability enforcement level** | The evidence Brain can honestly claim for a requested MCP or skill: `strictly-selected`, `advisory-only`, or `unavailable`. Logical allowlisting alone never upgrades the level. | `access::CapabilityEnforcement`; `brain skills status` |
| **session store** / **state DB** | The workspace-UUID-scoped SQLite DB (`~/.cache/brain/workspaces/<workspace-uuid>/state.db`) that tracks every registered frontend by frontend, workspace, actor, and channel (lock + recency). Claude validates transcripts, OpenCode validates live root sessions from the selected workspace, and Codex currently starts fresh. | `WorkspacePaths::state_db`; `src/state.rs`; `src/agent/opencode/session.rs` |
| **shared server** | The single machine-wide HTTP process elected by live TUIs. It has no independent daemon lifetime and exits after the final orderly unregister or final crashed-lease TTL. | `server::lifecycle`; `~/.cache/brain/server/` |
| **workspace lease** | A renewable, generation-bound claim that one exact workspace TUI and its UUID-local job socket are live. Receiver intent comes from the authoritative machine record. | `WorkspaceLease`; `LeaseTable`; `server::control` |
| **receiver ingress** | The portable opaque UUID in `/w/<ingress>/...` that selects a live workspace before any root, credential, user, prompt, log, or socket is read. | `IngressId`; `.config/workspace.json` |
| **receiver enablement** | Persistent selected-workspace intent. It does not mean a TUI or server exists; accepting requires enablement plus an unexpired exact-workspace lease. | `WorkspaceRecord.receiver_enabled`; `receiver_transition` |
| **unavailable response** | The one channel-specific response emitted when the shared process is alive but the selected target cannot accept. The message is discarded, not queued or replayed. | `server::receiver::unavailable` |
| **lifecycle bridges** | The generic session-start bridge that attributes a root frontend session to its workspace, actor, and channel, plus the generic turn-complete bridge that atomically publishes an authorized response. Claude and Codex invoke them as hooks; the thin OpenCode plugin translates `session.created` and `session.idle` events into the same payloads. | `scripts/agent_session_start_hook.py`; `scripts/agent_turn_complete_hook.py`; `scripts/opencode_brain_plugin.js` |
| **run.sh** | The entry-point script that rebuilds the binary when the sources change, then `exec`s it (no plan, no shell-side effects). | `run.sh` |
