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
| **local user** / **local actor** | The machine-local `local_user_id` attached to the selected context. The portable user registry and inbound actor override are planned later phases. | `WorkspaceRecord.local_user_id`; `WorkspaceContext::local_user_id()` |
| **workspace-only access** | A planned advisory mode based on prompt-based guidance and light guardrails. It is not a filesystem sandbox or an authentication boundary, and it is not enforced by the foundation release. | Later access-policy phase |

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
| **brain panel** | The always-available agent chat session in a PTY. Claude is the default; `--codex` / `-cx` selects Codex for the run. App-level: it does **not** belong to either main view and stays open across a main-view switch. (Formerly called the *claude panel* in the `tasks` project.) | `src/pty_pane.rs` (`PtyPane`); `App.brain: Option<PtyPane>`; `App.agent_kind` |
| **brain-panel tab** | Which session the brain panel is showing: the main session (`BrainTab::Main`) or the ephemeral **daily-triage** session (`BrainTab::Triage`), selected with `Alt+1` / `Alt+2`. The triage tab exists only while a daily-triage pass is running and is never tracked in the session DB. | `App.triage_brain: Option<PtyPane>`; `App.active_brain_tab: BrainTab`; `src/tui/app_triage_tab.rs` |
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
| **agent frontend** | Which CLI runs in the brain panel: Claude by default or Codex for a `--codex` / `-cx` shell. | `session::AgentKind`; `cli::Cli::agent_kind` |
| **the launch command** / **`claude_cmd` / `codex_cmd`** | The configured command the brain panel runs for the selected frontend. Both live in brain env because installed CLI paths and wrapper flags are machine-local; Claude gets `--resume`/`--session-id`, while Codex gets Codex-shaped args. | `env::claude_command`, `env::codex_command`, `session::build_llm_command` |
| **session store** / **state DB** | The workspace-UUID-scoped SQLite DB (`~/.cache/brain/workspaces/<workspace-uuid>/state.db`) that tracks Claude resume state (lock + recency); receiver completion hooks work for both Claude and Codex. Codex panels launch fresh. | `WorkspacePaths::state_db`; `src/state.rs` |
| **the hook** | The Claude `SessionStart` hook that attributes new sessions to this shell instance. | `scripts/claude_session_start_hook.py` |
| **run.sh** | The entry-point script that rebuilds the binary when the sources change, then `exec`s it (no plan, no shell-side effects). | `run.sh` |
