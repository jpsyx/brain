# Integrations

`brain` does almost nothing on its own that touches the outside world. It
discovers what the user wants and emits a **plan**; the zsh wrapper turns
that plan into real shell-side effects. This doc is the contract between
the two, plus the handoffs to claude and the `tasks` CLI.

## The plan protocol (`plan.rs` ↔ `brain` wrapper)

The binary prints lines of `key=value` to **stdout**. The wrapper captures
stdout, parses it line-by-line, and applies effects. Anything that isn't a
recognized directive is printed verbatim (so `brain --help` and clap
errors still work).

| Directive | Emitted by | Wrapper action |
| --- | --- | --- |
| `cd=<path>` | `plan::cd` (also after Finder reveal / file open) | `cd <path>` in the parent shell |
| `claude=<msg>` | `plan::claude` (always preceded by a `cd=`) | run the `cl` alias with `<msg>`; runs even when `<msg>` is empty |
| `open=<path>` | `plan::open` (non-text file, Ctrl-Enter) | `open <path>` (system default app) |
| `edit=<path>` | `plan::edit` (text file, Ctrl-Enter) | `${VISUAL:-${EDITOR:-vi}} <path>` |
| *anything else* | clap (help, version, errors) | printed verbatim |

(The old `tasks=1` directive is gone: the tasks view is now an in-process main
view of `brain`, not a separate binary the wrapper hands off to.)

Key contract points:

- **Stdout is the channel.** The TUI renders to `/dev/tty`, never stdout,
  so the captured plan is never garbled by terminal output. Diagnostics go
  to stderr.
- **The `claude=` directive is presence-keyed.** The wrapper opens claude
  whenever a `claude=` line is present, even with an empty value (that's
  the "open claude with no opening prompt" case). Tests pin this exact
  behavior (`claude_with_empty_message_still_emits_claude_directive`).
- **Order of application** in the wrapper: passthrough text, then `cd`,
  then `open`, then `edit`, then `claude`.
- **Wire strings are tested.** `plan.rs` has `*_to` variants writing into
  a buffer; unit tests assert the exact bytes (`"cd=/x\n"`, …). If you change
  a directive string, change it in `plan.rs`, this table, and the wrapper's
  `case` in the same edit.

## The tasks view (no more handoff)

Before the merge, `brain tasks` emitted `tasks=1` and the wrapper ran a
separate `tasks` zsh function/binary. That binary is gone. The tasks CSVs
(`~/brain/tasks/{tasks,habits}.csv`) are now read directly by `brain`'s tasks
main view (`crate::tasks`), and `brain tasks …` launches the merged shell (or
runs a tasks utility) in-process. The tasks-view side effects that *do* shell
out live in the tasks modules, not the plan protocol:

- **`~/global-skills/todo/scripts/mark_done.py`** — `brain tasks complete <id>`
  and the palette's mark-complete action `exec`/invoke it to mutate the CSVs.
- **`agenda` / `habits` zsh functions** — `Ctrl+A` (agenda) and the palette's
  "Open habits page" run these via the injected `ShellRunner`.
- **`cd ~/brain && claude …`** — the brain panel's PTY, shared by both main
  views (see below).

This is the heart of the "central dispatch" design: `brain` is the single
terminal command, and it routes to the right specialized tool (`tasks` for
task management, claude for conversational work, Finder/editor for files).

## Handoff: claude via the `cl` alias (one-shot `msg`)

`brain msg <prompt>` and the one-shot picker's "Message brain" item emit
`cd=~/brain` followed by `claude=<prompt>`. The wrapper resolves
`${aliases[cl]:-claude}` so it honors the user's `cl` alias (their
preferred claude invocation), falling back to bare `claude`. The message
is shell-quoted with `${(q)…}` before being passed.

## The brain panel: claude session + SessionStart hook + state DB

The persistent shell's brain panel is a **different** claude integration. It
spawns `claude` itself, inside a PTY (`pty_pane.rs`), running
`cd <root> && claude --resume <id>` or `--session-id <id>`
(`session::build_claude_command`). The PTY's working directory is **also**
set to `<brain_root>`, so claude resolves its project dir (and the
SessionStart hook in `.claude/settings.json`) under `~/brain` from the first
instant — every brain session is scoped to `~/brain`, so resume always looks
in the same place. It uses **bare `claude`**, not the `cl` alias, to control
the `--resume` / `--session-id` flag — same rationale as the `tasks` sibling.

Which session to run is decided by the **lock + recency** model in
`state.rs` (DB at `~/.cache/brain/state.db`, WAL):

1. On startup brain reaps locks held by dead PIDs, then walks
   `free_sessions_by_recency()` (sessions no live brain holds, newest first)
   and resumes the first whose **transcript actually exists** on disk —
   `~/.claude/projects/<mangled ~/brain>/<id>.jsonl`
   (`session::project_dir_name` + a fallback scan). A session opened but
   never chatted in leaves a DB row with **no** transcript, which `claude
   --resume` can't find (the "couldn't find session with ID …" error); brain
   skips those. If it claims a valid candidate it `--resume`s it; otherwise
   it starts a fresh `--session-id` (registered, locked to this PID) and, if
   it skipped a missing-transcript candidate, shows a status-line alert:
   *"couldn't find a session to resume — starting a new brain chat"*.
2. brain passes `BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` into
   the claude child's environment (`session::env_for`).
3. A **SessionStart hook** —
   `scripts/claude_session_start_hook.py`, wired into
   `~/brain/.claude/settings.json` under `hooks.SessionStart` — fires on
   every session start / resume / `/clear` / compact. Reading those env
   vars, it upserts the current session id (locked to `BRAIN_PID`) and frees
   the instance's other sessions, so a `/new` mid-run becomes the session
   brain resumes next time and the prior conversation stays resumable. With
   the env vars absent (ambient `~/brain` claude usage), the hook is a no-op.
4. When the panel closes (claude exits) or the shell quits, brain `release`s
   its lock, floating that session to the top of the resume queue — so
   "Message brain" (`Ctrl-M`) re-opens it, and a fresh startup resumes it.

**One hook, one DB, one namespace.** Before the merge, `brain` and `tasks`
each ran their own SessionStart hook keyed on separate env-var namespaces
(`BRAIN_*` vs `TASKS_*`) writing separate DBs, so the two shells never adopted
each other's sessions. The merged shell has a single app-level brain panel, so
there is now exactly **one** hook (`scripts/claude_session_start_hook.py`, keyed
on `BRAIN_*`), one DB (`~/.cache/brain/state.db`, table `brain_sessions`), and
one namespace. `scripts/install_hook.sh` installs it and strips the legacy
`tasks` SessionStart hook and the old `claude_stop_hook.py` Stop hook if
present. brain deliberately does **not** add a Stop hook — its brain-panel
sessions are continuous conversations, not discrete runs.

## System `open` and the editor

The two contexts open files differently:

- **Persistent shell** (the brain panel never closes): handled inside the
  running TUI by `open_target`'s impure spawners. A text file →
  `open_in_editor_tab`, which runs `osascript` to open a **new iTerm2 tab**
  (`iterm_new_tab_applescript` over `edit_shell_command` = `cd <dir> &&
  ${VISUAL:-${EDITOR:-nvim}} <file>`); a blob or directory →
  `open_with_system` (`open <path>`). On a non-iTerm2 terminal the editor
  path falls back to `open <file>`. Nothing is emitted to stdout; the shell
  stays up.
- **One-shot picker**: Finder reveal (`Ctrl-Enter`) calls the real `open`
  from inside the binary (`open_in_finder`) and emits a `cd=`. Direct open
  (`Enter`) is split by `open_target::is_textlike`: text → `edit=` (current
  terminal), else → `open=` (system app). Directories reveal in Finder.

## Handoff: `markdown-to-pdf` (the "Create PDF" command)

The "Create PDF" command (palette row / `Ctrl-G` on a `.md` file) converts the
highlighted markdown to a colocated same-name PDF and opens it. It reuses the
user's existing converter rather than reimplementing PDF generation.

`markdown-to-pdf` is a hard prerequisite. Its path is the config variable
`markdown_to_pdf_path`, auto-discovered on first run and validated at startup
(see [config.md](config.md) and `settings.rs`); a missing/invalid path fails
fast with a red error. `open_target::create_pdf` spawns that command directly
(`<file.md> --out <file.pdf>`) — invoking the command, not any shell-function
wrapper, since a child process can't call a shell function. The output path is
`open_target::pdf_output_path` (same directory, same stem, `.pdf`).

- **Same-name guarantee.** The converter's non-interactive mode does *not*
  overwrite an existing PDF — it writes a `-vN` variant. To keep the output
  name identical to the source, `create_pdf` removes any pre-existing PDF at
  the target path first, so the converter always writes the exact name.
- **Opening the result.** In the **persistent shell** the conversion runs in
  place and the PDF is handed to `open_target::open_with_system` (`open
  <pdf>`) — the brain shell stays up. In the **one-shot picker** the binary
  runs the conversion, then emits an `open=<pdf>` directive so the wrapper
  opens it after `brain` exits.
- **Best-effort.** In the persistent shell a converter failure is swallowed
  (like a failed file-open) so a broken toolchain can't tear the shell down.

## Handoff: `osascript` → Finder trash (the "Delete" command)

The "Delete" command (palette row / `Ctrl-D` on any entry) moves the
highlighted file or directory to the **Trash** rather than unlinking it, so a
mistaken delete is recoverable (`Put Back`). It's a **user-style** delete: no
new mechanism, just the same Trash the user empties by hand.

`open_target::move_to_trash` shells out to `osascript` with the line
`open_target::trash_applescript` builds — `tell application "Finder" to delete
POSIX file "<path>"` (the path escaped for the AppleScript literal). Finder's
`delete` handles both files and directories and lands them in the Trash.

- **Confirmed first.** Both entry points route through the red `confirm.rs`
  modal (default **No**); the trash only runs on `Accept`.
- **Refresh after.** The persistent shell re-walks its scope (`App::refresh`);
  the one-shot picker drops the trashed path in memory
  (`picker::App::drop_path`). Either way the entry disappears from the list.
- **Best-effort.** A failed `osascript` is swallowed (like the PDF path) so a
  denied automation permission can't tear the shell down.

## The auto-rebuild

The wrapper rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then runs it. This is the only
reason an agent's source edit "takes effect" without a manual
`cargo build` — but you should still build/test explicitly while
developing (see [testing.md](testing.md)).
