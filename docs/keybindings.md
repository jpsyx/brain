# Keybindings

The merged `brain` shell has **two main views** (the tasks view and the
brain-directory search view) and one app-level **brain panel** (the `claude`
PTY). See [glossary.md](glossary.md) for the vocabulary. Startup: the **tasks
view** is showing, the **brain panel is open** (on the right) but unfocused, so
`j`/`k` work immediately.

Keys are resolved in this precedence (see `tui/event_loop.rs`):

1. **App-level accelerators** — intercepted before everything, from either
   view: `Ctrl+Q` quit, `Alt+S` help, `Alt+H/L` panel focus, `Alt+U/D` scroll,
   `Ctrl+X` close brain, `Ctrl+N` new session, and (main-panel-focused only)
   the view-switch chords `Ctrl+H/L`, `Ctrl+T`, `Ctrl+B`.
2. **Modal overlays** — a captive modal (help / palette / confirm / brain-input
   / link-picker) consumes the key.
3. **The brain panel** — when focused, keys forward to `claude` as bytes.
4. **The active main view** — the tasks handlers, or the brain-search picker.

## App-level (work in either main view)

| Key | Action | Notes |
| --- | --- | --- |
| `Ctrl+L` / `Ctrl+H` | Cycle the main view right / left | Two views today, so both wrap to the other; the direction is kept for a future third view. Main-panel focus only, so the brain panel keeps Claude's `Ctrl+H` (backspace) etc. when it has focus |
| `Ctrl+T` | Jump to the **tasks** view | Main-panel focus only |
| `Ctrl+B` | Jump to the **brain-directory** view | Main-panel focus only |
| `Alt+H` / `Alt+L` | Focus the **left** / **right** panel | Spatial: follows the layout when the brain panel is swapped sides. `Alt+H` from the brain panel is the reliable way back to the main view |
| `Alt+U` / `Alt+D` | Scroll the focused panel a half-page up / down | Brain panel scrolls its scrollback; the main view pages. Fires while Claude has focus or a filter is active |
| `Ctrl+M` | Open (or focus) the brain panel | Resumes the latest session. Needs the kitty protocol to stay distinct from Enter |
| `Ctrl+N` | Start a new Claude session in the brain panel | Types `/new` and submits it. Only while the panel is open |
| `Ctrl+X` | Close the brain panel (ends its claude session) | The main view goes full-width. From either panel |
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
browser is now the palette's **"Open habits page"** row (the old `Ctrl+H`
binding became the cycle-view accelerator).

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

An always-filtering fuzzy picker over `~/brain` (projects / areas / resources
/ archive). Every printable key edits the query.

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
(`menu.rs`) is separate from the tasks palette; its own confirm overlays
(PDF / delete) are captive while open.

## Modals

Shared across the app; a captive modal consumes all input.

- **Help** (`Alt+S`) — the `shortcuts::ALL` reference, grouped. `j/k`, `PgUp/PgDn`, `g`, `?`/`q`/`Esc` close.
- **Command palette** (`Ctrl+P`, tasks view) — filterable; numbered rows; `Enter` runs, `Esc` closes.
- **Task actions** (`Enter` on a task) — per-task command list.
- **Confirm** — Yes/No (triage adds Skip). `y`/`n`/`Esc`, `←`/`→`/`Tab` move, `Enter` resolves.
- **Brain-input** (`Ctrl+Shift+M`) — compose a seeded message. `Alt+Enter` newline, `Enter` send.
- **Link picker** (`Ctrl+O`, ≥ 2 links) — numbered; digit opens, `Enter` opens highlighted.

## Kitty keyboard protocol

`run_tui` requests `DISAMBIGUATE_ESCAPE_CODES`. With it, `Ctrl+M`/`Ctrl+Enter`
are distinct from `Enter`, `Ctrl+H`/`Ctrl+L` from Backspace/Tab-family, and
`Ctrl+Shift+M` reports its Shift. Without it (legacy Terminal.app):

- `Ctrl+M` / `Ctrl+Enter` / `Ctrl+Shift+M` collapse to `Enter` → use the palette.
- `Ctrl+H` collapses to Backspace, so **cycle-view-left is unavailable**; use `Ctrl+L` (right) or the palette. `Ctrl+T` / `Ctrl+B` / `Ctrl+L` have no aliasing.
- `Alt+S`, `Ctrl+D`, `Ctrl+A`, `Ctrl+Q`, `Ctrl+X`, `Ctrl+N` are all reliable (Meta sequence or control bytes with no aliasing).
