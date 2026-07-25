# `brain` docs — index

> **For LLM agents:** Read this directory before changing code in
> `path/to/brain/`. Every command, menu item, keybinding, and
> integration with the outside world is documented here. **If you add a
> feature or change architecture, update these docs in the same change.**
> See `../AGENTS.md` for the contract.

`brain` is the user's **central terminal dispatch** for everything around
their second brain and task system: manage tasks (agenda, triage, habits),
fuzzy-pick a note across the PARA buckets, or think with an always-on
`claude` brain panel. Bare `brain` opens a **persistent shell** with two main
views (tasks and brain-directory search) alongside a session-resuming
`claude` brain panel. The only subcommands are `brain tasks …` and `brain
config`.

This directory is the source-of-truth for *what* `brain` does and *why*.
The code is the source-of-truth for *how*. They must agree on *what*.

## Read order

0. **[glossary.md](glossary.md)** — plain-English term → code mapping
   (main view, sub-view, brain panel, the two switching axes). Read first.
1. **[architecture.md](architecture.md)** — module map, the merged-shell
   routing, data flow, build/run loop.
2. **[features.md](features.md)** — every user-visible capability: the two
   main views, subcommands, the fuzzy picker, the tasks view.
3. **[data-model.md](data-model.md)** — `Bucket`, `Entry`, the
   `HaystackBuf` normalization, the picker's match/row model.
4. **[keybindings.md](keybindings.md)** — the app-level, tasks-view, and
   brain-search-view key tables, plus the kitty-protocol caveat.
5. **[integrations.md](integrations.md)** — `run.sh`, the brain panel's
   `claude` launch (`claude_cmd`), the file-open / Finder / PDF / trash
   handoffs, the tasks-view shell-outs, and the SessionStart hook / state DB.
6. **[config.md](config.md)** — the config store, the `brain config`
   command, the `markdown-to-pdf` prerequisite, and root resolution.
7. **[testing.md](testing.md)** — the red/green TDD doctrine, what we
   test (and deliberately don't), and the test layout.
8. **[decisions.md](decisions.md)** — the "why" behind the non-obvious
   choices: `/dev/tty` rendering, kitty flags, slug normalization, the
   config-driven `claude` launch, the central-dispatch framing.

## Source layout (quick map)

```
src/
  main.rs        — entry point, command dispatch (bare brain → tasks view)
  lib.rs         — public re-exports for integration tests
  cli.rs         — clap surface (Cli + Cmd: tasks / config)
  config.rs      — typed knobs (triage pattern, linear, rollover, claude_cmd)
  paths.rs       — brain-root resolution (config store / $HOME, tilde expand)
  settings/      — config store + `brain config` + markdown-to-pdf prereq
  entry.rs       — Bucket + Entry; walkdir collection with hidden filter
  tui/           — persistent shell (tasks view + search view + claude panel)
  pty_pane.rs    — PTY-backed brain panel (portable-pty + vt100)
  session.rs     — pure claude command/env + resume-vs-fresh plan
  state.rs       — SQLite session store + layout pref (lock + recency)
  picker/        — ratatui fuzzy picker (matching, grouping, navigation)
  menu/          — ratatui command palette (Ctrl-p overlay)
  render.rs      — pure functions → styled ratatui Lines (picker UI)
  open_target.rs — "how to open this path" + new-iTerm2-tab opener
scripts/
  claude_session_start_hook.py — records the live Claude session id
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
| How does the brain panel launch `claude`? | integrations.md → "The brain panel" |
| Why is `brain` a pure TUI binary with no wrapper? | decisions.md |
| How do I point `brain` at a different root? | config.md |
| How do I add a test the right way? | testing.md |
