# `brain` docs — index

> **For LLM agents:** Read this directory before changing code in
> `~/src/jpsyx/brain/`. Every command, menu item, keybinding, and
> integration with the outside world is documented here. **If you add a
> feature or change architecture, update these docs in the same change.**
> See `../AGENTS.md` for the contract.

`brain` is the user's **central terminal dispatch** for everything around
their second brain and task system: cd between PARA buckets, fuzzy-pick a
note across them, think with claude, or jump into the `tasks` TUI (task
management, agenda, triage). Bare `brain` opens a **persistent two-panel
shell** — fuzzy search alongside an always-on, session-resuming `claude`
brain panel; the subcommands stay one-shot.

This directory is the source-of-truth for *what* `brain` does and *why*.
The code is the source-of-truth for *how*. They must agree on *what*.

## Read order

0. **[glossary.md](glossary.md)** — plain-English term → code mapping
   (main view, sub-view, brain panel, the two switching axes). Read first.
1. **[architecture.md](architecture.md)** — module map, the merged-shell
   routing, the plan protocol, data flow, build/run loop.
2. **[features.md](features.md)** — every user-visible capability: the two
   main views, subcommands, the fuzzy picker, the tasks view.
3. **[data-model.md](data-model.md)** — `Bucket`, `Entry`, the
   `HaystackBuf` normalization, the picker's match/row model.
4. **[keybindings.md](keybindings.md)** — the app-level, tasks-view, and
   brain-search-view key tables, plus the kitty-protocol caveat.
5. **[integrations.md](integrations.md)** — the zsh wrapper, the
   `cd`/`claude`/`open`/`edit` directives, the tasks-view shell-outs, and
   the unified SessionStart hook / state DB.
6. **[config.md](config.md)** — `config.json` schema and root
   resolution order.
7. **[testing.md](testing.md)** — the red/green TDD doctrine, what we
   test (and deliberately don't), and the test layout.
8. **[decisions.md](decisions.md)** — the "why" behind the non-obvious
   choices: the plan protocol, `/dev/tty` rendering, kitty flags, slug
   normalization, the central-dispatch framing.

## Source layout (quick map)

```
src/
  main.rs        — entry point, command dispatch (bare brain → tui::run)
  lib.rs         — public re-exports for integration tests
  cli.rs         — clap surface (Cli + Cmd + QueryArgs)
  paths.rs       — brain-root resolution (config.json / $HOME, tilde expand)
  entry.rs       — Bucket + Entry; walkdir collection with hidden filter
  tui.rs         — persistent two-panel shell (search + claude brain panel)
  pty_pane.rs    — PTY-backed brain panel (portable-pty + vt100)
  session.rs     — pure claude command/env + resume-vs-fresh plan
  state.rs       — SQLite session store + layout pref (lock + recency)
  picker.rs      — ratatui fuzzy picker (matching, grouping, navigation)
  menu.rs        — ratatui command palette (Ctrl-p overlay)
  render.rs      — pure functions → styled ratatui Lines (picker UI)
  open_target.rs — "how to open this path" + new-iTerm2-tab opener
  plan.rs        — emit shell-side directives to stdout (the wire protocol)
scripts/
  claude_session_start_hook.py — records the live Claude session id
tests/
  entry_collect.rs   — entry::collect against real temp dir trees
  root_resolution.rs — config parse + tilde expansion composition
brain            — the zsh wrapper function (user's entry point)
config.json      — runtime config (see config.md)
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
| What does the binary print and who runs it? | integrations.md → "Plan protocol" |
| Why doesn't `brain` just `cd` itself? | decisions.md |
| How do I point `brain` at a different root? | config.md |
| How do I add a test the right way? | testing.md |
