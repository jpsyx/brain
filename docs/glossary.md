# Glossary

Plain-English terms the user says, mapped to the Rust identifiers / modules
they name. When the user refers to something by an everyday word, look it up
here to find the code. Keep this in sync when you rename a concept.

## The two-axis layout model

The merged `brain` shell has **two main views** and **one app-level panel**.
At most one main view shows at a time; the brain panel can be open alongside
it (split) or closed (main view full-width).

| Plain English | What it means | Code |
| --- | --- | --- |
| **main view** | One of the two full-screen surfaces you switch between. | `main_view::MainView` (`src/main_view.rs`) |
| **tasks view** | The task-management surface (agenda, triage, habits). The **default** at startup. | `MainView::Tasks`; state in the tasks App fields / `src/tasks/` modules |
| **brain directory view** / **brain search view** | The fuzzy-search-over-`~/brain` surface (this was *bare `brain`* before the merge). | `MainView::BrainSearch`; `src/picker/`, `src/entry.rs` |
| **brain panel** | The always-available agent chat session in a PTY. Claude is the default; `--codex` selects Codex for the run. App-level: it does **not** belong to either main view and stays open across a main-view switch. (Formerly called the *claude panel* in the `tasks` project.) | `src/pty_pane.rs` (`PtyPane`); `App.brain: Option<PtyPane>`; `App.agent_kind` |
| **panel** | Generic term; in this app the only panel is the brain panel. | — |
| **sub-view** | One of the tabbed modes *inside* the tasks view (`today`, `mit`, `past_due`, `week`, `habits`, `backlog`, `all`). These were called "views" in the old `tasks` project. `Tab` / `Shift+Tab` cycle them; only meaningful in the tasks view. | `view::View` + `View::CYCLE` (`src/tasks/view/`) |
| **focus** | Which surface receives keystrokes: the active main view, or the brain panel. `Alt+H`/`Alt+L` move focus between the spatial left/right halves. | `App.focus` |

## View switching vs. panel focus (two different axes)

These are deliberately distinct and use different modifiers:

| Plain English | Effect | Keys | Code |
| --- | --- | --- | --- |
| **cycle main views** | Change *which main view* is shown (tasks ↔ brain directory). | `Ctrl+H` (left) / `Ctrl+L` (right) | `main_view::ctrl_cycles_view` → `MainView::step` |
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
| **agent frontend** | Which CLI runs in the brain panel: Claude by default or Codex for a `--codex` shell. | `session::AgentKind`; `cli::Cli::agent_kind` |
| **the launch command** / **`claude_cmd` / `codex_cmd`** | The configured command the brain panel runs for the selected frontend. `claude_cmd` lives in brain config and gets `--resume`/`--session-id`; `codex_cmd` lives in brain env and gets Codex-shaped args. | `config::Config::claude_command`, `env::codex_command`, `session::build_llm_command` |
| **session store** / **state DB** | The SQLite DB (`~/.cache/brain/state.db`) that tracks which Claude session the brain panel resumes (lock + recency). Codex panels launch fresh until brain has a Codex hook/store. | `src/state.rs` |
| **the hook** | The Claude `SessionStart` hook that attributes new sessions to this shell instance. | `scripts/claude_session_start_hook.py` |
| **run.sh** | The entry-point script that rebuilds the binary when the sources change, then `exec`s it (no plan, no shell-side effects). | `run.sh` |
