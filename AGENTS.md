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
interactive agent session in a PTY — plus a tab per running **skill session**,
each a single-prompt ephemeral session that closes itself — running this machine's
`default_agent_frontend` env value — Claude unless set — and overridden for one
run by `--claude` / `-cl`, `--codex` / `-cx`, or `--open-code` / `-oc`, open at
startup and shared by all views). Switch views with
`Ctrl+L`/`Ctrl+H` (cycle) or
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
Ordinary commands resolve one immutable `WorkspaceContext` and `ActorContext`;
agent work flows through `AgentController`. The persistent shell keeps
UUID-scoped state
(`~/.cache/brain/workspaces/<workspace-uuid>/state.db`, table
`brain_sessions`) for frontend session locks, completion delivery, and panel
layout. Claude validates transcripts and its live-session registry, OpenCode
validates exact-root live sessions, and Codex validates its exact on-disk
rollout; each resumes an eligible workspace-scoped session and starts fresh
when its evidence is missing. All three use the same frontend-neutral state and completion schema
through registry-declared lifecycle integrations.
One machine-wide shared HTTP process exists only for the lifetime of live TUI
leases and stops after the final orderly close or expired crashed lease.

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
| A **main-view-switch** or app-level keybinding (`Ctrl+H/L/T/B`, `Alt+S`) | `docs/keybindings.md`, the pure classifiers in `src/main_view.rs`, and the Global rows in `src/tasks/shortcuts.rs` |
| A **brain-search-view** keybinding or menu row | `docs/keybindings.md`, `src/menu/model.rs` (`items` + `shortcut_for`), `src/tui/search_view.rs` |
| How the brain panel launches Claude, Codex, or OpenCode (`claude_cmd`, `codex_cmd`, `opencode_cmd`, frontend selectors), or the file-open / Finder path | `docs/integrations.md` (controller/adapters in `src/agent/`; compatibility builders in `src/session.rs`; agent commands in `src/env/`; openers in `src/open_target.rs`) |
| The session-start/session-stop bridges, frontend registry, frontend-neutral state DB schema, or `BRAIN_*` env | `docs/integrations.md`, `scripts/{agent_session_start_hook,agent_session_stop_hook}.py`, `scripts/opencode_brain_plugin.js`, `src/agent/registry.rs`, `src/agent/registry/contract.rs`, `src/command/server/receiver/hooks.rs`, `src/state.rs` |
| Brain-config schema, the `brain config` command, or the config dir location (`<brain-root>/.config/`) | `docs/config.md` (store + schema in `src/settings/`; typed knobs in `src/config.rs`) |
| Brain-env schema, the `brain env` command, the `markdown-to-pdf` prerequisite, `claude_cmd`, `codex_cmd`, `default_agent_frontend`, the `sync` block's fields, or **root resolution** (`root` is structural workspace-registry data in `~/.config/brain/env.json`, never writable free-form env; the legacy `~/.config/brain-root` pointer is read-only migration input) | `docs/config.md` + `docs/data-model.md` (env store + schema + migration in `src/env/`; legacy compatibility in `src/paths.rs`; selected roots in `src/workspace/`; the `sync` block schema in `src/sync/config.rs`) |
| `brain sync` itself (the `sync`/`--push`/`--pull`/`setup`/`repair`/`status`/`conflicts` surface), the rclone bisync transport, keep-both conflict naming, the `--max-delete` guard, or the sync journal | `docs/features.md` + `docs/integrations.md` + `docs/architecture.md` + `docs/data-model.md` (pipeline in `src/sync/`: `config`, `remote`, `args`, `run`, `conflicts`, `verify`, `journal`, `setup`, `command`; dispatched before the gate in `src/main.rs`) |
| The `tasks.csv`/`habits.csv` schema-aware semantic merge (excluding them from bisync, the baseline cache, merge/reconciliation rules, or the journal's `csv:` note) | `docs/features.md` + `docs/integrations.md` + `docs/data-model.md` + `docs/decisions.md` (pure merge in `src/sync/csv_merge/`; baseline + rclone `copyto` transport + orchestration in `src/sync/csv_sync/`; wired into `src/sync/command/mod.rs::sync_once`; excludes in `src/sync/args.rs`) |
| The auto-sync triggers (startup pull, change-triggered push, receiver freshness pull, the `notify` watcher + debounce, the sync lock) | `docs/features.md` + `docs/architecture.md` + `docs/integrations.md` + `docs/decisions.md` (modules `src/sync/{freshness,lock,trigger,watch}.rs`; `debounce_ms` in `src/sync/config.rs`; `format_triggers` in `src/sync/command/mod.rs`; seams in `src/tui/{app_sync,event_loop/setup}.rs`) |
| Remote sync identity, empty-remote initialization, or explicit legacy-remote UUID adoption | `docs/config.md` + `docs/features.md` + `docs/integrations.md` + `docs/data-model.md` + `docs/decisions.md` (`src/sync/{identity,setup,remote}.rs`; all data lanes consume `VerifiedRemote`) |
| `brain workspace migrate`, its compatibility/readiness gates, journal, retained backup, task-schema activation, or recovery behavior | `docs/config.md` + `docs/features.md` + `docs/architecture.md` + `docs/integrations.md` + `docs/data-model.md` + `docs/decisions.md` (`src/migration/`; coordinator-owned task activation in `src/tasks/schema/`) |
| **Skill sessions** — the `skill_sessions` env array, the builtin daily-triage definition, a session's tab lifecycle, the appended completion protocol, or the session-done route/signal | `docs/features.md` + `docs/config.md` + `docs/data-model.md` + `docs/integrations.md` + `docs/architecture.md` + `docs/glossary.md` + `docs/keybindings.md` + `docs/decisions.md` (pure model/prompt/signal/editor in `src/skill_session/`; tab lifecycle in `src/tui/app_skill_session/`; route in `src/server/routes/session/`; launch env in `src/session/mod.rs`; palette rows in `src/tui/palette/`) |
| Required workspace availability or optional per-workspace feature health (`off`/`ready`/`incomplete`) | `docs/config.md` + `docs/features.md` + `docs/data-model.md` + `docs/testing.md` (`src/workspace/requirements/`; selected inspectors reload only their UUID-pinned record) |
| The second-brain sync skill rows (`cloud-sync`, `resolve-conflicts`), `brain sync conflicts --json`, and `brain sync resolve` | `docs/features.md` + `docs/integrations.md` + `docs/data-model.md` + `docs/architecture.md` + `docs/decisions.md` (`parse_conflict_name`/`group_conflicts`/`copies_for_original` in `src/sync/conflicts.rs`; `conflicts_json` in `src/sync/command/mod.rs`; `resolve` in `src/sync/command/resolve.rs`; the bundled rows in `skills/second-brain/SKILL.md`) |
| The persona schema (per-user identity, `namespaces`, tag styles), its user-ID keying or schema migration, the `brain persona` command, the missing-persona prompt, the namespace/tag checklist, tag-style defaults, or the brain config dir (`<brain-root>/.config/`) | `docs/config.md` + `docs/data-model.md` (one persona in `src/personalization/persona.rs`; the keyed store + migration in `src/personalization/personas.rs`; store IO in `src/personalization/store.rs`; namespaces in `src/personalization/namespaces.rs`; tag defaults in `src/personalization/tags.rs`; checklist in `src/personalization/checklist/`; the prompt gate in `src/personalization/onboarding.rs`) |
| The interactive `brain config set <var>` mode (checklist for `namespaces`/`tags`, value prompt for scalars) | `docs/config.md` (dispatch in `src/main.rs` `config_set_interactive`; personalization editors in `src/personalization/command.rs`) |
| The skill pipeline (bundling, rendering, install/fan-out, `brain skills sync`, `resync_skills()`, the `skills_auto_sync` gate) | `docs/architecture.md` + `docs/features.md` + `docs/decisions.md` (pipeline in `src/skills/`; bundled skills under `skills/`) |
| The brain HTTP server (`brain server {status\|logs}`, the elected shared process + `~/.cache/brain/server/` state, `brain habits`, the `/habits` route, or a **new server endpoint / web view**) | `docs/architecture.md` + `docs/features.md` + `docs/integrations.md` (server in `src/server/`: `router` for path dispatch, `lifecycle` for election and process ownership, `routes/<name>/` per-endpoint MVC; add a route module + one `routes/mod.rs` line, never a giant endpoints file; web views under `web/<name>/`, embedded via `include_str!`) |
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

## Automatic migrations

Users never run migrations themselves. Every `brain` invocation except help or
version runs all applicable machine migrations before ordinary dispatch. A
migration must clean up the superseded state, transform existing state, and
create any missing state needed by the target version. The current migration
is also reconciled on later invocations so deleted or stale managed artifacts
self-heal.

Every migration added from now on must provide both `up` and `down` operations.
Keep migrations small and modular under `src/startup_migration/`. `install.sh`
must detect an existing Brain version and invoke the new binary for an upgrade
or the existing binary for a downgrade before replacement. Help and version
must remain side-effect free.

## Build, run, test

```sh
# rebuild (when src/ changed) + run via run.sh, which execs the binary
brain

# manual rebuild
( cd path/to/brain && cargo build --release )

# headless smoke test (no TUI on these paths)
./target/release/brain workspace list
./target/release/brain config list -b brain
./target/release/brain sync status -b family
./target/release/brain receiver status -b family
./target/release/brain server status
./target/release/brain tasks today --no-tui -b brain

# the full test suite — runs in well under a second
( cd path/to/brain && cargo test --release )

# one module's unit tests / one integration file
cargo test --release picker::
cargo test --release --test entry_collect

# lint clean (pedantic + nursery are on)
cargo clippy --release --all-targets -- -D warnings
```

The `brain` command runs `run.sh`, which builds the binary when the sources
change and `exec`s it. The user never types `cargo run`.

`install.sh` is the other entry point: it builds release and installs the binary
at `$BIN_DIR/brain` (default `~/.local/bin`). Keep it **idempotent** — a fixed
filename, overwritten in place, so it doubles as the updater and never leaves a
second copy — and keep it working **for anyone who clones this repo**, on a
machine that knows nothing about it. When a prerequisite is missing or the
result won't be usable, say so and print the fix (no Rust toolchain, a `BIN_DIR`
that isn't on `$PATH`). Never let a stranger's install fail silently or leave a
command they can't invoke.

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

- **Skill code and core skill text know nothing about extensions.** A bundled
  skill (`skills/<name>/SKILL.md`) declares hooks (`<!-- brain:ext <skill>:<hook> -->`)
  that a user's extension *may* fill; the skills pipeline renders them into a
  flattened copy (markers stripped, content inlined or appended — see
  `src/skills/extension.rs`). When you touch **anything skill-related** — a
  bundled `SKILL.md`, the skills pipeline, or any code that coordinates with a
  skill (the triage completion signal, a hook-driven handoff) — you may assume
  only that *a hook might carry extension content*. Never assume an extension
  exists, never assume what it contains, and **never bake an
  extension-specific artifact into core** — no path, filename, directory, URL,
  or product name (`~/Downloads`, an "agenda", a "PDF", a private endpoint) may
  appear in core code or core skill text unless it is genuinely part of the
  generic core. Any generic mechanism core provides for extensions must have a
  correct **no-op default** when no extension contributes (an empty list, a
  skipped step), so the bundled core and any fork behave identically with no
  extensions installed. When a run must hand core data an extension produced
  (e.g. "these output files must exist before the tab closes"), pass it as
  **runtime data the run supplies**, not an author-time assumption in core. The
  daily-triage completion signal's `require` gate (`src/triage_signal.rs`,
  `docs/decisions.md`) is the reference implementation.
- **No `unsafe`.** `[lints.rust] unsafe_code = "forbid"` enforces it.
- **Keep clippy clean.** `pedantic` + `nursery` are on at `warn`; don't
  add new warnings.
- **The binary's stdout is only intentional machine-readable/plain CLI output:**
  `config/env/version`, `workspace list`, the `receiver` details listing and the
  `receiver email` / `receiver phone` addresses, explicit plain-task output,
  help, and non-TUI logs mirrored by `--verbose`. Clap errors and diagnostics go
  to stderr. The TUI renders to `/dev/tty`. Never `println!` diagnostics from
  other paths.
- **Don't add dependencies casually.** The set is small on purpose. If you
  need one, justify it in `docs/architecture.md`.
- **Follow the pure/impure split.** New decision logic goes in a pure
  function (testable); the `/dev/tty` and `Command` shells stay thin.
- **One module per file; keep files small.** Prefer many small,
  single-responsibility modules over a few giant ones, the Rust analogue of
  the one-component-per-file convention. A `.rs` file that grows past **~400
  lines of production code** (inline `#[cfg(test)]` blocks don't count toward
  the production budget, but remain subject to modularity review) is a smell:
  split it into a directory of submodules
  (`foo.rs` → `foo/mod.rs` + `foo/<part>.rs`), the way `src/tasks/` already
  does with `task/`, `render/`, and `view/`. Split along real seams
  (matching vs. model vs. render; store vs. schema vs. discovery; one handler
  group per file), not at an arbitrary line. `mod.rs` should stay a thin
  re-export + glue layer, not a dumping ground. Apply the same standard to unit
  tests, test-only modules, integration tests, benchmarks, examples, test
  support, harnesses, and fixtures. Split large or multi-responsibility suites
  by behavior or subsystem, and build fixtures from focused, composable helpers
  instead of a catch-all fake process, server, builder, or support module.
  Don't split a file that's already cohesive just to hit a number; the
  400-line figure is a prompt to look, not a hard cap.
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
  output (`brain sync`, `sync setup`/`status`, `config`, `env`, `persona`,
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
  The **one exception** is a project-management-log-only commit (see the next
  rule): it touches no source, triggers no release, and must **not** bump the
  version.
- **Every LLM capability must flow through `AgentController` and work with
  Claude, Codex, and OpenCode.** When adding or changing brain-panel behavior,
  implement and test equivalent lifecycle, prompt, completion, and delivery
  behavior for every registered frontend. If one exposes a different
  integration surface
  (for example, Claude settings versus Codex workspace `.codex/hooks.json`), bridge the
  difference inside Brain. Keep every frontend behind the same controller
  facade; never route around the controller to make one call site appear
  supported.
- **Auto-commit project-management-log-only changes straight to `main`.** When
  a change is **exclusively** an update to the project-management log under
  `docs/product-manager/` made through the `/repo-product-manager` skill (a new
  task, a triage edit, a cycle plan, a closed/archived task, etc.) and touches
  **nothing else**, commit and push it to `main` automatically, without asking.
  This is a standing authorized Git exception (it overrides the default
  "don't commit/push" posture) precisely because it is not product work — it is
  bookkeeping. The rules:
  - **Markdown only, no source.** The commit may contain **only**
    `docs/product-manager/**` changes (and its `media/` attachments). If the
    working tree also has any source, doc, or config change, it is **not** a
    PM-only change: do not auto-commit; leave everything for normal review.
  - **No version bump, no release.** Do **not** touch `Cargo.toml` /
    `Cargo.lock`; a PM-log commit never triggers a version bump or a release
    (the sole exception to the version-bump rule above).
  - **Never mix it into the working branch.** The PM update must **not** land on
    whatever branch or worktree you are currently on. Treat it as a brief
    detour: stash or set aside the PM changes as needed, commit them to `main`
    and push, then return to the exact branch/worktree you were on and resume
    the real work. The current branch's diff must look afterward as if the PM
    commit never happened there.
  - **Commit message.** Use a `docs(pm):` (or `chore(pm):`) subject naming the
    task id(s) touched, e.g. `docs(pm): add BR-3 …`.
  - **Scope.** This applies only to `docs/product-manager/` bookkeeping. Any
    change that is *about the product* (source, tests, product docs under
    `docs/` outside `product-manager/`) follows the normal branch + review +
    version-bump flow, even when a `/repo-product-manager` step accompanies it.

## CLI ↔ command-palette state parity

**Any toggleable TUI state must be reachable from both the CLI (at startup) and
the command palette (at runtime), and the two must stay in lock-step.** The TUI
can stay open for days, so a setting a user picked at launch may need flipping
mid-session, and a setting they flip mid-session is one they may want to launch
straight into next time. Neither surface is complete on its own.

The **startup half** is a CLI *surface*, not necessarily a `Cli` flag. It is
either a global flag (`--with-receiver`) or a declared `brain config` / `brain
env` variable set from the CLI (`brain config set
enable_daily_triage_check=false`). Pick between them with the same test that
decides every other variable — see "brain env vs. brain config" in
[docs/config.md](docs/config.md): a value every machine on the workspace should
agree on is portable config; a per-machine choice is env; a genuinely
one-invocation choice is a flag. A value that belongs in a store must not also
be a flag: two persistence stories for one setting is exactly the divergence
this rule exists to prevent.

Concretely, whenever you add either half, add the other in the same change:

- **New startup surface that affects live TUI state** (a boolean or small mode
  that the running app reads from an `App` field, e.g.
  `enable_daily_triage_check` → `App::skip_daily_triage_check`) → add a
  **global command-palette command** that toggles that same `App` field for the
  current session. If the state is binary, the palette row's label is dynamic:
  it names the action that will happen next (`Disable daily triage alert` when
  currently enabled, `Enable daily triage alert` when currently disabled),
  mirroring the existing Start/Stop-style toggle rows.
- **New command-palette command that mutates the TUI's internal configuration
  state** → add an equivalent **startup surface** (a declared config/env
  variable, or a flag when it really is per-invocation) so the brain can *start*
  in that state on launch, threaded through `main.rs` → `run_tui` → `App::new`
  the same way the existing state is.

The startup surface and the palette toggle must write the *same* `App` field
through the *same* pure decision, so the two paths can never diverge. A
config-seeded toggle also **persists** the flip: silencing something from the
palette is the same decision as `brain config set …`, so it survives a restart
and reaches the workspace's other machines. Write the store *and* the live field
in one action, and if the write fails, keep honoring it for the running session
while saying so — silently degrading a persistent choice to a session-only one is
the surprise this rule exists to prevent. Keep the palette label
registered in both palette surfaces (`src/tui/palette/` and, for global rows,
`src/menu/model.rs`) and the docs (`docs/features.md`, `docs/keybindings.md`) in
sync, per the docs contract.
