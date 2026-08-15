# `brain` docs — index

> **For LLM agents:** Read this directory before changing code in
> `path/to/brain/`. Every command, menu item, keybinding, and
> integration with the outside world is documented here. **If you add a
> feature or change architecture, update these docs in the same change.**
> See `../AGENTS.md` for the contract.

`brain` is the user's **central terminal dispatch** for everything around
their second brain and task system: manage tasks (agenda, triage, habits),
fuzzy-pick a note across the PARA buckets, or think with a live agent
brain panel. Bare `brain` opens a **persistent shell** with three main views
(tasks, brain-directory search, and logs) alongside a session-resuming agent
brain panel. `brain workspace …` manages the machine's workspace registry;
the other command families operate on one selected workspace.
Brain's execution surfaces are a persistent TUI and short-lived command
families.

This directory is the source-of-truth for *what* `brain` does and *why*.
The code is the source-of-truth for *how*. They must agree on *what*.

## Read order

0. **[glossary.md](glossary.md)** — plain-English term → code mapping
   (main view, sub-view, brain panel, the two switching axes). Read first.
1. **[architecture.md](architecture.md)** — module map, the merged-shell
   routing, data flow, build/run loop.
2. **[features.md](features.md):** every user-visible capability: the three
   main views, subcommands, the fuzzy picker, the tasks view.
3. **[data-model.md](data-model.md)** — `Bucket`, `Entry`, the
   `HaystackBuf` normalization, the picker's match/row model.
4. **[keybindings.md](keybindings.md)** — the app-level, tasks-view, and
   brain-search-view key tables, plus the kitty-protocol caveat.
5. **[integrations.md](integrations.md):** `run.sh`, `AgentController`, the
   Claude/Codex/OpenCode launch adapters, shared TUI-lifetime server, workspace
   sync/migration boundaries, file handoffs, and frontend hooks / state DB.
6. **[config.md](config.md)** — the config store, the `brain config`
   command, the `markdown-to-pdf` prerequisite, and root resolution.
7. **[testing.md](testing.md)** — the red/green TDD doctrine, what we
   test (and deliberately don't), and the test layout.
8. **[decisions.md](decisions.md)** — the "why" behind the non-obvious
   choices: `/dev/tty` rendering, kitty flags, slug normalization, the
   registry-driven agent facade, and the central-dispatch framing.

## Source layout (quick map)

```
src/
  main.rs        : entry point, workspace bootstrap, and command dispatch
  lib.rs         — public re-exports for integration tests
  cli/           : focused clap surface (global + command-family modules)
  workspace/     : WorkspaceContext, schema-v2 registry, requirements, and commands
  actor/         : immutable ActorContext for local and authenticated requests
  agent/         : AgentController, frontend registry, and Claude/Codex/OpenCode adapters
  users/         : portable people, normalized identities, and atomic users.json storage
  startup_migration/ : automatic machine up/down migrations and reconciliation
  migration/     : explicit journaled legacy-to-multi-workspace rollout
  config.rs      — typed knobs (triage pattern, linear, rollover)
  paths.rs       : legacy migration-only root compatibility
  settings/      — config store + `brain config` + markdown-to-pdf prereq
  entry.rs       — Bucket + Entry; walkdir collection with hidden filter
  tui/           : persistent shell (tasks, search, and logs views + agent panel)
  pty_pane.rs    — PTY-backed brain panel (portable-pty + vt100)
  session.rs     : compatibility re-exports over the frontend-neutral agent layer
  state.rs       : UUID-scoped SQLite sessions, completion, and metadata
  sync/          : UUID-scoped runtime, remote identity, triggers, and CSV merge
  picker/        — ratatui fuzzy picker (matching, grouping, navigation)
  menu/          — ratatui command palette (Ctrl-p overlay)
  render.rs      — pure functions → styled ratatui Lines (picker UI)
  open_target.rs — "how to open this path" + new-iTerm2-tab opener
scripts/
  agent_session_start_hook.py  : frontend-neutral attributed session rotation
  agent_session_stop_hook.py   : frontend-neutral authorized completion publication
  opencode_brain_plugin.js     : thin OpenCode event-to-bridge adapter
tests/
  entry_collect.rs   — entry::collect against real temp dir trees
  root_resolution.rs — config parse + tilde expansion composition
run.sh           — builds when sources change, then execs the binary
config.example.json — sample config; the real store is <brain-root>/.config/ (see config.md)
docs/            — this directory
AGENTS.md        — agent contract (root)
CLAUDE.md        — symlink → AGENTS.md
```

## When to read which doc

| Question | Doc |
| --- | --- |
| Where does X live? | architecture.md |
| What does pressing Ctrl-Enter do in the picker? | keybindings.md |
| What is "Open tasks"? | features.md → "Open tasks" |
| How does `ann-afloat` match the query `afloat`? | data-model.md → "HaystackBuf" |
| How does the brain panel launch an agent frontend? | integrations.md → "The Brain Panel" |
| Why does `brain` need no plan protocol or wrapper? | decisions.md |
| How do I create, attach, or select a workspace? | config.md and features.md |
| How do I add a test the right way? | testing.md |
