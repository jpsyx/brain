# Keybindings

The merged `brain` shell has **three main views** (tasks, brain-directory
search, and logs) and one app-level **brain panel** (the selected
agent PTY: Claude by default, Codex with `--codex` / `-cx`). See [glossary.md](glossary.md) for the vocabulary. Startup: the **tasks
view** is showing, the **brain panel is open** (on the right) but unfocused, so
`j`/`k` work immediately.

Keys are resolved in this precedence (see `tui/event_loop/run.rs`):

1. **App-level accelerators** — intercepted before everything, from either
   view: `Ctrl+Q` quit, `Alt+S` help, `Alt+H/L` panel focus, `Alt+U/D` scroll,
   `Ctrl+X` close brain, `Ctrl+N` new session, and (main-panel-focused only)
   the view-switch chords `Ctrl+H/L`, `Ctrl+T`, `Ctrl+B`.
2. **Modal overlays** — a captive modal (help / palette / confirm / brain-input
   / link-picker / assignee-filter picker) consumes the key.
3. **The brain panel** — when focused, keys forward to the selected agent as bytes.
4. **The active main view** — the tasks handlers, or the brain-search picker.

## App-level (work in either main view)

| Key | Action | Notes |
| --- | --- | --- |
| `Ctrl+L` / `Ctrl+H` | Cycle the main view right / left | Cycles tasks, brain search, and logs. Main-panel focus only, so the brain panel keeps Claude's `Ctrl+H` (backspace) etc. when it has focus |
| `Ctrl+T` | Jump to the **tasks** view | Main-panel focus only |
| `Ctrl+B` | Jump to the **brain-directory** view | Main-panel focus only |
| `Alt+H` / `Alt+L` | Focus the **left** / **right** panel | Spatial: follows the layout when the brain panel is swapped sides. `Alt+H` from the brain panel is the reliable way back to the main view |
| `Alt+U` / `Alt+D` | Scroll the focused panel a half-page up / down | Brain panel scrolls its scrollback; the main view pages. Fires while the selected agent has focus or a filter is active. Also accepts macOS Option-produced equivalents when richer keyboard reporting surfaces those instead of Alt-modified ASCII |
| `Ctrl+M` | Open (or focus) the brain panel | Resumes the latest Claude session; Codex panels currently launch fresh. Needs the kitty protocol to stay distinct from Enter |
| `Ctrl+N` | Start a new agent session in the brain panel | Types `/new` and submits or queues it. Only while the panel is open |
| `Alt+[` / `Alt+]` | Cycle the brain-panel tab (previous / next): **main** session ↔ **daily-triage** session | The reliable switch (resolves as Alt-modified brackets or the macOS Option smart-quote glyphs). No-op unless a daily-triage tab is open. The command palette (`Ctrl+P`) also carries **Show main brain session** / **Show daily triage session**. `Alt+1` / `Alt+2` still select a tab directly on terminals that support Alt+digit, but that encoding is unreliable, hence the bracket cycle. From either panel |
| `Ctrl+X` | Close the brain panel (ends its agent session) | The main view goes full-width. From either panel. **On the daily-triage tab it closes only that ephemeral session**, leaving the main session up |
| `Alt+S` | Open the keyboard-shortcuts help modal | Replaces the old bare `?`; bound to `Alt+S` so a literal `?` still types into the brain-search filter. Distinct Meta sequence on every terminal |
| `Ctrl+Q` | Unconditional quit | Intercepted before modals/panels; quits even from the brain panel or a modal. `0x11`, no kitty protocol needed |

**Panel focus vs. view switching** are two different axes: `Alt+H/L` move
*focus* between the main view and the brain panel; `Ctrl+H/L` change *which
main view* is shown. Both read as "left/right" but mean different things.

## Tasks view

The tasks view is a vim-style modal list with tabbed sub-views. It is the
startup default.

### Normal mode

| Key | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Next / previous task (accepts a count prefix, e.g. `3j`) |
| `d` / `u` | Half-page down / up |
| `PgDn` / `PgUp` | Full page down / up |
| `g` / `G` (`Home` / `End`) | First / last task |
| `→` / `←` | Expand / collapse the highlighted entry's notes |
| `l` | Toggle the selected entry's notes (preview ↔ full markdown) |
| `Tab` / `Shift+Tab` | Cycle **sub-view** forward / backward (today → mit → past_due → week → habits → backlog → all) |
| `t` `m` `p` `w` `h` `b` `a` | Jump to sub-view (today/mit/past_due/week/habits/backlog/all). Bare letters only. `h` collapses notes instead when the highlighted entry's notes are expanded |
| `/` | Enter search mode (live fuzzy filter) |
| `r` | Reload `tasks.csv` + `habits.csv` |
| `Enter` | Open the task actions modal for the selected entry |
| `Ctrl+D` | Mark the selected task complete (confirm modal). `0x04`, no kitty protocol needed |
| `Ctrl+Backspace` | Remove the selected task (confirm modal) — tasks only. Bare Backspace is a no-op |
| `Ctrl+O` | Open the selected entry's links (Linear issue + notes URLs) |
| `Ctrl+Enter` | Open the task actions modal (mainly for search mode) |
| `Ctrl+P` | Open the command palette (global + task commands) |
| `Ctrl+Shift+M` | Brain-input modal seeded with the selected task as context |
| `Ctrl+A` | Open today's agenda (offers to generate it when missing) |
| `q` / `Esc` | Quit (Esc clears an active filter first). Also `Ctrl+C` |

`Ctrl+P`, `Ctrl+A`, `Ctrl+Shift+M`, and the task actions are **tasks-view
only** (gated on `main_view == Tasks`). Opening today's habits page in the
browser is now the palette's **"Open habits in browser"** row (served by the
bundled brain server; the old `Ctrl+H` binding became the cycle-view
accelerator).

### Search mode (`/` active)

| Key | Action |
| --- | --- |
| printable char | Append to the query |
| `Backspace` | Delete (empty query → exit search) |
| `Ctrl+U` | Clear the query (empty query → exit search) |
| `Enter` | Exit search mode, keep the filter |
| `Esc` / `Ctrl+C` | Cancel the filter, exit search (does not quit) |
| `Ctrl+Enter` | Open the task actions modal for the selected entry |
| other `Ctrl+<key>` | Falls through to the normal-mode shortcut |

## Brain-directory (search) view

An always-filtering fuzzy picker over the selected workspace's projects,
areas, resources, and archive directories. Every printable key edits the
query.

| Key | Action |
| --- | --- |
| printable char | Append to the query and refilter |
| `Backspace` / `Ctrl+U` / `Ctrl+W` | Delete char / clear / delete word |
| `↑` / `↓` (`Ctrl+K` / `Ctrl+J`) | Move selection |
| `PgUp` / `PgDn` / `Home` / `End` | Page / jump |
| `Enter` | Open the highlighted entry in place (text → editor tab, blob → system open, dir → Finder) — shell stays up |
| `Ctrl+Enter` | Reveal the entry in Finder |
| `Ctrl+G` | Create a PDF from the highlighted `.md` file (green confirm modal) |
| `Ctrl+D` | Delete the highlighted entry (red confirm modal → Trash) |
| `Ctrl+R` | Refresh the list (re-walk the current scope, keep the query) |
| `Ctrl+P` | Open the brain-search command palette (rescope, layout, message brain, open tasks, PDF/delete/open) |
| `Esc` / `Ctrl+C` | Quit the shell |

`Tab` / `Shift+Tab` do nothing here (no sub-views). The brain-search palette
(`menu/`) is separate from the tasks palette; its own confirm overlays
(PDF / delete) are captive while open.

## Modals

Shared across the app; a captive modal consumes all input.

- **Help** (`Alt+S`) — the `shortcuts::ALL` reference, grouped. `j/k`, `PgUp/PgDn`, `g`, `?`/`q`/`Esc` close.
- **Command palette** (`Ctrl+P`, tasks view) — filterable; numbered rows;
  `Enter` runs, `Esc` closes. In `--verbose` TUI runs it includes **Show
  logs**, which asks whether to reveal the timestamped `/tmp` log file. It
  also includes **Sync brain now**, **Show sync status**, and a
  **Disable/Enable daily triage alert** toggle (the session-scoped counterpart
  to `--no-daily-triage-check`), all with no direct shortcut. In a shared
  workspace it also includes **Add task** and **Filter by assignee**; both are
  intentionally palette-only.
- **Task actions** (`Enter` on a task): per-task command list. Shared
  workspaces add the palette-only **Reassign this task** row.
- **Confirm** — Yes/No (the daily-triage nudge adds **Skip**, which marks today's
  Morning Triage habit done deterministically in-process — no agent). `y`/`n`/`s`/`Esc`,
  `←`/`→`/`Tab` move, `Enter` resolves.
- **Brain-input** (`Ctrl+Shift+M`) — compose a seeded message. `Alt+Enter` newline, `Enter` send.
- **Link picker** (`Ctrl+O`, ≥ 2 links) — numbered; digit opens, `Enter` opens highlighted.
- **Assignee filter** (shared-workspace palette row): numbered portable
  members plus **All assignees**; digit or `Enter` applies, `Esc` closes. The
  active member appears below the task-view heading, and task-view `Esc`
  clears it before quitting.

## Kitty keyboard protocol

`run_tui` requests `DISAMBIGUATE_ESCAPE_CODES`. With it, `Ctrl+M`/`Ctrl+Enter`
are distinct from `Enter`, `Ctrl+H`/`Ctrl+L` from Backspace/Tab-family, and
`Ctrl+Shift+M` reports its Shift. Without it (legacy Terminal.app):

- `Ctrl+M` / `Ctrl+Enter` / `Ctrl+Shift+M` collapse to `Enter` → use the palette.
- `Ctrl+H` collapses to Backspace, so **cycle-view-left is unavailable**; use `Ctrl+L` (right) or the palette. `Ctrl+T` / `Ctrl+B` / `Ctrl+L` have no aliasing.
- `Alt+S`, `Alt+U`, `Alt+D`, `Ctrl+D`, `Ctrl+A`, `Ctrl+Q`, `Ctrl+X`, `Ctrl+N` are all reliable (Meta sequence, macOS Option-glyph fallback, or control bytes with no aliasing).
