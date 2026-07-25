# AGENTS.md

You are an AI agent working in the `brain` repo. This file is the
single entry point for anything an agent needs to know about the project.
**Read it before changing code.** (Other agent tools — Codex, opencode,
Cursor — also resolve `AGENTS.md`; `CLAUDE.md` is a symlink to this file
so Claude Code picks it up too.)

## What this project is

`brain` is the user's **central terminal dispatch** and the **single CLI**
for both their second brain and their task system. (The standalone `tasks`
binary was merged in; it no longer exists.) It's a small Rust CLI
(clap + ratatui) that browses `~/brain` (PARA) and manages
`~/brain/tasks/{tasks,habits}.csv`.

Bare `brain` (and `brain tasks …`) opens a **persistent shell** (`tui/`) with
**two main views** — the **tasks view** (task management, agenda, triage; the
startup default) and the **brain-directory search view** (fuzzy-pick over
`~/brain`) — plus one app-level **brain panel** (an interactive `claude`
session in a PTY that resumes your latest conversation, open at startup and
shared by both views). Switch views with `Ctrl+L`/`Ctrl+H` (cycle) or
`Ctrl+T`/`Ctrl+B` (jump). Read [docs/glossary.md](docs/glossary.md) first for
the main-view / sub-view / brain-panel vocabulary.

The only subcommands are `brain tasks …` (open the tasks view, or run the
tasks utilities `complete|doctor|search|--no-tui`) and `brain config
{list|get|set}`; bare `brain` opens the shell on the tasks view (the startup
default). There are **no** shell-mutating one-shot commands and **no** plan
protocol, so `brain` needs no wrapper: `run.sh` builds the binary when the
sources change and `exec`s it directly. Everything the user does happens
inside the TUI, which renders to `/dev/tty` and performs its own file-open,
Finder-reveal, and `claude`-launch actions by spawning processes; the
binary's stdout carries only `brain config` output plus clap help/errors. The
persistent shell keeps state (`~/.cache/brain/state.db`, table
`brain_sessions`) to resume the right Claude session (lock + recency) and
remember the panel layout, fed by a single Claude `SessionStart` hook
(`scripts/claude_session_start_hook.py`, keyed on `BRAIN_*`).

For a deeper map, see [docs/architecture.md](docs/architecture.md).

## The docs/ contract

**Whenever you add a feature, change a keybinding, change how the brain
panel launches `claude`, change root resolution, or change the module
shape, update the relevant file under `docs/` in the same change.**

The docs are the source-of-truth for *what* `brain` does and *why*. Code
is the source-of-truth for *how*. They must agree on *what*.

| If you change… | Update… |
| --- | --- |
| A plain-English term ↔ code mapping (view, sub-view, panel, …) | `docs/glossary.md` |
| Module list, data flow, subcommand/main-view routing | `docs/architecture.md` |
| User-visible behavior (main views, menu items, subcommands, picker/tasks behavior) | `docs/features.md` |
| `Bucket` / `Entry`, the picker match model, the `Task`/sub-view model | `docs/data-model.md` |
| A **tasks-view** keybinding | `docs/keybindings.md`, the `src/tasks/shortcuts.rs` table (footer + help modal), `compact_footer_line` in `src/tasks/render/chrome.rs`, **and** (if it's also a palette / task-action row) `shortcut_for` in `src/tui/palette/command.rs` |
| A **main-view-switch** or app-level keybinding (`Ctrl+H/L/T/B`, `Alt+?`) | `docs/keybindings.md`, the pure classifiers in `src/main_view.rs`, and the Global rows in `src/tasks/shortcuts.rs` |
| A **brain-search-view** keybinding or menu row | `docs/keybindings.md`, `src/menu/model.rs` (`items` + `shortcut_for`), `src/tui/search_view.rs` |
| How the brain panel launches `claude` (`claude_cmd`), or the file-open / Finder path | `docs/integrations.md` (launch builder in `src/session.rs`, `claude_cmd` in `src/config.rs`/`src/settings/`, openers in `src/open_target.rs`) |
| The SessionStart hook, state DB schema, or `BRAIN_*` env | `docs/integrations.md`, `scripts/claude_session_start_hook.py`, `scripts/install_hook.sh`, `src/state.rs` |
| Config schema, the `brain config` command, the `markdown-to-pdf` prerequisite, the config dir location (`<brain-root>/.config/`), or **root resolution** (the `~/.config/brain-root` pointer / `~/brain` default; root is *not* a config var) | `docs/config.md` (store + schema + discovery in `src/settings/`; typed knobs in `src/config.rs`; root in `src/paths.rs`) |
| The personalization schema (identity, `namespaces`, tag styles), the `brain personalize` command, first-run onboarding, the namespace/tag checklist, tag-style defaults, or the brain config dir (`<brain-root>/.config/`) | `docs/config.md` + `docs/data-model.md` (schema/store in `src/personalization/`; namespaces in `src/personalization/namespaces.rs`; tag defaults in `src/personalization/tags.rs`; checklist in `src/personalization/checklist/`) |
| The interactive `brain config set <var>` mode (checklist for `namespaces`/`tags`, value prompt for scalars) | `docs/config.md` (dispatch in `src/main.rs` `config_set_interactive`; personalization editors in `src/personalization/command.rs`) |
| The skill pipeline (bundling, rendering, install/fan-out, `brain skills sync`, `resync_skills()`, the `skills_auto_sync` gate) | `docs/architecture.md` + `docs/features.md` + `docs/decisions.md` (pipeline in `src/skills/`; bundled skills under `skills/`) |
| Testing strategy, what we test vs. skip | `docs/testing.md` |
| A non-obvious design choice | `docs/decisions.md` |

If a change spans categories, update all the relevant docs. Do not defer.

## Red/Green TDD — the iron law

**No production code lands without a failing test written first.**

1. **RED.** Write the smallest test for the next behavior. Run it. Watch
   it fail. A test you never saw fail proves nothing.
2. **GREEN.** Write the simplest code that turns *that* test green. Don't
   add behavior the red test doesn't demand; don't widen the test surface
   between red and green.
3. **REFACTOR.** Clean up with the bar green; re-run to stay green.

Then the next red. When fixing a bug: first a failing test that reproduces
it, *then* the fix. Push every decision worth testing into a **pure
function** (the picker's matching, menu `handle_key`, `is_textlike`,
config parsing, `session::build_claude_command`) and test that, rather than
mocking the terminal or the filesystem. See
[docs/testing.md](docs/testing.md) for the full strategy and layout.

## Build, run, test

```sh
# rebuild (when src/ changed) + run via run.sh, which execs the binary
brain

# manual rebuild
( cd path/to/brain && cargo build --release )

# headless smoke test (no TUI on these paths)
./target/release/brain config list        # → the config table
./target/release/brain tasks today --no-tui  # → today's tasks, plain text

# the full test suite — runs in well under a second
( cd path/to/brain && cargo test --release )

# one module's unit tests / one integration file
cargo test --release picker::
cargo test --release --test entry_collect

# lint clean (pedantic + nursery are on)
cargo clippy --release --all-targets
```

The `brain` command runs `run.sh`, which builds the binary when the sources
change and `exec`s it. The user never types `cargo run`.

## Quick orientation for new agents

1. Read [docs/README.md](docs/README.md) — the index.
2. Read [docs/architecture.md](docs/architecture.md) end-to-end.
3. Scan [docs/features.md](docs/features.md) for what the user can do.
4. Open the file you actually need to touch.
5. RED test → GREEN code → update docs, in the same change.

## House rules (project-specific)

- **No `unsafe`.** `[lints.rust] unsafe_code = "forbid"` enforces it.
- **Keep clippy clean.** `pedantic` + `nursery` are on at `warn`; don't
  add new warnings.
- **The binary's stdout is only `brain config` output (plus clap
  help/errors).** Never `println!` diagnostics from other paths. Diagnostics
  go to stderr; the TUI goes to `/dev/tty`.
- **Don't add dependencies casually.** The set is small on purpose. If you
  need one, justify it in `docs/architecture.md`.
- **Follow the pure/impure split.** New decision logic goes in a pure
  function (testable); the `/dev/tty` and `Command` shells stay thin.
- **One module per file; keep files small.** Prefer many small,
  single-responsibility modules over a few giant ones — the Rust analogue of
  the one-component-per-file convention. A `.rs` file that grows past **~400
  lines of production code** (inline `#[cfg(test)]` blocks don't count toward
  the budget) is a smell: split it into a directory of submodules
  (`foo.rs` → `foo/mod.rs` + `foo/<part>.rs`), the way `src/tasks/` already
  does with `task/`, `render/`, and `view/`. Split along real seams
  (matching vs. model vs. render; store vs. schema vs. discovery; one handler
  group per file), not at an arbitrary line. `mod.rs` should stay a thin
  re-export + glue layer, not a dumping ground. When a file is large only
  because of inline tests, split the *tests* by area instead. Don't split a
  file that's already cohesive just to hit a number — the 400-line figure is a
  prompt to look, not a hard cap.
- **Keep the dimmed shortcut annotation in sync with the binding.** Every
  command palette row that has a direct keystroke shows it as a gray `[…]`
  hint, driven by `shortcut_for` in `src/menu/model.rs`. Whenever you add or
  change a keybinding for an action that also appears in the palette,
  update `shortcut_for` in the same change so the gray hint matches the
  real binding. If I tell you a new (or changed) action's shortcut is
  `[abc]`, register `[abc]` in `shortcut_for` as part of the work. Don't
  make me ask for it.
- **Comments only when the *why* is non-obvious.** The function name and
  the docs cover the *what*.
- **`docs/` is the durable record.** The repo is under git, but we keep no
  `.difit/` decision-log file: design rationale goes in `docs/decisions.md`,
  not a per-branch scratch file.
