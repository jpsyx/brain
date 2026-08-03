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
(clap + ratatui) that browses a selected brain workspace (PARA) and manages
that root's `tasks/{tasks,habits}.csv`.

Bare `brain` (and `brain tasks …`) opens a **persistent shell** (`tui/`) with
**three main views**: the **tasks view** (task management, agenda, triage; the
startup default) and the **brain-directory search view** (fuzzy-pick over the
selected root), plus the **logs view**, and one app-level **brain panel** (an
interactive agent session
in a PTY, Claude by default or Codex with `--codex` / `-cx`, open at startup and
shared by all views). Switch views with `Ctrl+L`/`Ctrl+H` (cycle) or
`Ctrl+T`/`Ctrl+B` (jump). Read [docs/glossary.md](docs/glossary.md) first for
the main-view / sub-view / brain-panel vocabulary.

The short-lived command families include `brain tasks …` utilities,
`brain config …`, `brain env …`, `brain workspace …`, sync, personalization,
skills, server/receiver management, habits, checks, and reindexing; bare
`brain` opens the shell on the tasks view (the startup default). There are
**no** shell-mutating one-shot commands and **no** plan protocol, so `brain`
needs no wrapper: `run.sh` builds the binary when the sources change and
`exec`s it directly. The intentional stdout families are
`config/env/version`, `workspace list`, explicit plain-task output, and help.
`--verbose` mirrors logs to stdout for non-TUI commands. Clap errors and
diagnostics go to stderr. The TUI renders to `/dev/tty` and performs its own
file-open, Finder-reveal, and agent-launch actions by spawning processes. The
persistent shell keeps UUID-scoped state
(`~/.cache/brain/workspaces/<workspace-uuid>/state.db`, table
`brain_sessions`) to resume the right Claude session (lock + recency) and
remember the panel layout. Claude sessions are fed by a single Claude
`SessionStart` hook (`scripts/claude_session_start_hook.py`, keyed on
`BRAIN_*`); Codex panels launch fresh until brain has a Codex session hook.

For a deeper map, see [docs/architecture.md](docs/architecture.md).

## The docs/ contract

**Whenever you add a feature, change a keybinding, change how the brain
panel launches an agent frontend, change root resolution, or change the module
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
| How the brain panel launches Claude or Codex (`claude_cmd`, `codex_cmd`, `--codex` / `-cx`), or the file-open / Finder path | `docs/integrations.md` (launch builder in `src/session.rs`, agent launch commands in `src/env/`, openers in `src/open_target.rs`) |
| The SessionStart hook, state DB schema, or `BRAIN_*` env | `docs/integrations.md`, `scripts/claude_session_start_hook.py`, `scripts/install_hook.sh`, `src/state.rs` |
| Brain-config schema, the `brain config` command, or the config dir location (`<brain-root>/.config/`) | `docs/config.md` (store + schema in `src/settings/`; typed knobs in `src/config.rs`) |
| Brain-env schema, the `brain env` command, the `markdown-to-pdf` prerequisite, `claude_cmd`, `codex_cmd`, the `sync` block's fields, or **root resolution** (`root` is structural workspace-registry data in `~/.config/brain/env.json`, never writable free-form env; the legacy `~/.config/brain-root` pointer is read-only migration input) | `docs/config.md` + `docs/data-model.md` (env store + schema + migration in `src/env/`; legacy compatibility in `src/paths.rs`; selected roots in `src/workspace/`; the `sync` block schema in `src/sync/config.rs`) |
| `brain sync` itself (the `sync`/`--push`/`--pull`/`setup`/`repair`/`status`/`conflicts` surface), the rclone bisync transport, keep-both conflict naming, the `--max-delete` guard, or the sync journal | `docs/features.md` + `docs/integrations.md` + `docs/architecture.md` + `docs/data-model.md` (pipeline in `src/sync/`: `config`, `remote`, `args`, `run`, `conflicts`, `verify`, `journal`, `setup`, `command`; dispatched before the gate in `src/main.rs`) |
| The `tasks.csv`/`habits.csv` id-keyed semantic merge (excluding them from bisync, the baseline cache, the merge rules, or the journal's `csv:` note) | `docs/features.md` + `docs/integrations.md` + `docs/data-model.md` + `docs/decisions.md` (pure merge in `src/sync/csv_merge.rs`; baseline + rclone `copyto` transport + orchestration in `src/sync/csv_sync.rs`; wired into `src/sync/command.rs::sync_once`; excludes in `src/sync/args.rs`) |
| The auto-sync triggers (startup pull, change-triggered push, receiver freshness pull, the `notify` watcher + debounce, the sync lock) | `docs/features.md` + `docs/architecture.md` + `docs/integrations.md` + `docs/decisions.md` (modules `src/sync/{freshness,lock,trigger,watch}.rs`; `debounce_ms` in `src/sync/config.rs`; `format_triggers` in `src/sync/command/mod.rs`; seams in `src/tui/{app_sync,event_loop/setup}.rs`) |
| The second-brain sync skill rows (`cloud-sync`, `resolve-conflicts`), `brain sync conflicts --json`, and `brain sync resolve` | `docs/features.md` + `docs/integrations.md` + `docs/data-model.md` + `docs/architecture.md` + `docs/decisions.md` (`parse_conflict_name`/`group_conflicts`/`copies_for_original` in `src/sync/conflicts.rs`; `conflicts_json` in `src/sync/command/mod.rs`; `resolve` in `src/sync/command/resolve.rs`; the bundled rows in `skills/second-brain/SKILL.md`) |
| The personalization schema (identity, `namespaces`, tag styles), the `brain personalize` command, first-run onboarding, the namespace/tag checklist, tag-style defaults, or the brain config dir (`<brain-root>/.config/`) | `docs/config.md` + `docs/data-model.md` (schema/store in `src/personalization/`; namespaces in `src/personalization/namespaces.rs`; tag defaults in `src/personalization/tags.rs`; checklist in `src/personalization/checklist/`) |
| The interactive `brain config set <var>` mode (checklist for `namespaces`/`tags`, value prompt for scalars) | `docs/config.md` (dispatch in `src/main.rs` `config_set_interactive`; personalization editors in `src/personalization/command.rs`) |
| The skill pipeline (bundling, rendering, install/fan-out, `brain skills sync`, `resync_skills()`, the `skills_auto_sync` gate) | `docs/architecture.md` + `docs/features.md` + `docs/decisions.md` (pipeline in `src/skills/`; bundled skills under `skills/`) |
| The brain HTTP server (`brain server {start\|status\|kill}`, the background daemon + `~/.cache/brain/server.json` state, `brain habits`, the `/habits` route, or a **new server endpoint / web view**) | `docs/architecture.md` + `docs/features.md` + `docs/integrations.md` (server in `src/server/`: `router` for path dispatch, `lifecycle` for the daemon, `routes/<name>/` per-endpoint MVC — add a route module + one `routes/mod.rs` line, never a giant endpoints file; web views under `web/<name>/`, embedded via `include_str!`) |
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
config parsing, `session::build_llm_command`) and test that, rather than
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

## Agent dev-skills (pinned per-repo, not global)

This repo pins the agent **development** skills contributors should use (so
everyone gets the same guidance regardless of what's installed globally). They
live in `skills-lock.json` (committed); the materialized copies under
`.agents/` / `.claude/skills/` / `.windsurf/` are gitignored ("node_modules" of
the `skills` CLI). After cloning, restore them with:

```sh
npx skills experimental_install     # materializes the pinned skills locally
```

Currently pinned (all directly relevant to this Rust, TDD-first repo):

- **`rust-skills`** (leonardomso/rust-skills) — 265 idiomatic-Rust rules; invoke
  with `/rust-skills` when writing/reviewing/refactoring Rust here.
- **`test-driven-development`** (obra/superpowers) — the red/green loop this
  repo's "iron law" requires.
- **`systematic-debugging`** (obra/superpowers) — for any bug/test-failure.

Add more with `npx skills add <owner>/<repo>@<skill>` (project scope by default)
and commit the updated `skills-lock.json`. Do **not** rely on globally-installed
skills — pin what this repo needs here. (These are *developer* skills for people
working **on** brain; they are separate from the product skills brain ships to
users in `skills/`.)

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
- **The binary's stdout is only intentional machine-readable/plain CLI output:**
  `config/env/version`, `workspace list`, explicit plain-task output, help, and
  non-TUI logs mirrored by `--verbose`. Clap errors and diagnostics go to
  stderr. The TUI renders to `/dev/tty`. Never `println!` diagnostics from
  other paths.
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
- **Terminal output always prioritizes aesthetics and user-friendliness.**
  This is a top-priority, non-negotiable principle for brain: every terminal-
  facing surface should be as pleasant and clear as we can make it — brain aims
  to be the *easiest terminal tool to use, ever*. Two audiences, both first-class:
  - **LLM-friendly:** there is a **CLI flag / subcommand for every possible
    action**, so an agent can drive brain fully non-interactively (no action is
    reachable *only* through an interactive prompt).
  - **Human-friendly:** when a human omits a required value, brain **drops into an
    interactive mode** (a themed prompt / guided walkthrough) instead of erroring
    — e.g. `brain sync setup` with no flags walks you through it, but the same
    values can be passed as flags. Never make a human guess or read `--help` to
    do the common thing.
  When you add or change any command, provide both paths (flags for everything +
  an interactive fallback for missing values) and make the output beautiful.
- **Default output should narrate long-running work.** `--verbose` is for
  detailed logs and debugging, not for basic reassurance. If a command may spend
  noticeable time checking files, touching the network, acquiring locks,
  probing tools, merging data, or waiting on a child process, print a concise
  themed line before each phase that says what brain is about to do and, when
  useful, which local root / remote / file family it is looking at. A user
  should never wonder whether brain is hung. Keep this deterministic and
  factual: it is a progress trace, not a debug dump or hidden reasoning.
- **Aesthetics matter — theme every bit of CLI output.** brain's non-TUI CLI
  output (`brain sync`, `sync setup`/`status`, `config`, `env`, `personalize`,
  `doctor`, gates, prompts) should look considered, not utilitarian. All color
  goes through the **`src/theme.rs` `Theme` semantic tokens** — `heading`,
  `accent`, `value`, `muted`, `success`, `warning`, `error`, `info`, `prompt` —
  chosen for *meaning*, never a raw ANSI escape inline. When you add or change
  CLI output, style it with the token that matches its role (a success message
  is `theme.success`, a command name is `theme.accent`, a hint is `theme.muted`,
  an interactive prompt label is `theme.prompt`, …), and be tasteful: color
  guides the eye, it doesn't paint everything. Get the theme via
  `Theme::active()` (color auto-gated off when stderr isn't a TTY or `NO_COLOR`
  is set); pass `Theme::dark(false)` in tests that assert on plain text.
  - **Design for dark terminals.** Terminals don't reliably expose a light/dark
    token, so we assume **dark** and use bright, high-contrast codes (never a
    dark foreground like plain blue `34` that vanishes on a dark background). A
    guard test enforces this. Adding a light (or other) theme later is just a
    new `Theme::…()` code table plus the selection in `Theme::active` — so keep
    all color decisions *in* `Theme`, never scattered as literals.
- **`docs/` is the durable record.** The repo is under git, but we keep no
  `.difit/` decision-log file: design rationale goes in `docs/decisions.md`,
  not a per-branch scratch file.
- **Bump the crate version for every committed change.** `Cargo.toml` is the
  single version source and `Cargo.lock` must move with it. Choose the SemVer
  bump yourself: before v1, additive user-visible features bump the minor
  version, and compatible fixes/internal changes bump the patch version. Do not
  ask for confirmation; the user will say when `brain` is ready for `1.0.0`.
- **Every LLM capability must work with both Claude and Codex.** When adding
  or changing brain-panel behavior, implement and test equivalent lifecycle,
  prompt, completion, and delivery behavior for both frontends. If one agent
  exposes a different integration surface (for example, Claude settings versus
  Codex `~/.codex/hooks.json`), bridge the difference inside brain rather than
  silently leaving one frontend unsupported.

## CLI ↔ command-palette state parity

**Any toggleable TUI state must be reachable from both the CLI (at startup) and
the command palette (at runtime), and the two must stay in lock-step.** The TUI
can stay open for days, so a setting a user picked at launch may need flipping
mid-session, and a setting they flip mid-session is one they may want to launch
straight into next time. Neither surface is complete on its own.

Concretely, whenever you add either half, add the other in the same change:

- **New CLI option that affects live TUI state** (a boolean or small mode that
  the running app reads from an `App` field, e.g. `--no-daily-triage-check` →
  `App::skip_daily_triage_check`) → add a **global command-palette command**
  that toggles that same `App` field for the current session. If the state is
  binary, the palette row's label is dynamic: it names the action that will
  happen next (`Disable daily triage alert` when currently enabled,
  `Enable daily triage alert` when currently disabled), mirroring the existing
  Start/Stop-style toggle rows.
- **New command-palette command that mutates the TUI's internal configuration
  state** → add an equivalent **CLI option** so the brain can *start* in that
  state on launch, threaded through `main.rs` → `run_tui` → `App::new` the same
  way the existing flags are.

The startup flag and the palette toggle must write the *same* `App` field
through the *same* pure decision, so the two paths can never diverge. Keep the
palette label registered in both palette surfaces (`src/tui/palette/` and, for
global rows, `src/menu/model.rs`) and the docs (`docs/features.md`,
`docs/keybindings.md`) in sync, per the docs contract. Process-scoped runtime
state (not persisted config) is the default for these toggles unless the value
clearly belongs in `brain config`.
