# Integrations

`brain` is a single binary that owns its own outside-world effects. It has no
shell-mutating one-shot commands, so there is no plan protocol and no zsh
wrapper: everything the user does happens inside the persistent TUI, which
opens files, reveals in Finder, converts PDFs, trashes entries, and launches
`claude` by spawning processes itself. This doc covers how the binary is run
and each of those handoffs, plus the SessionStart hook and state DB.

## How brain is run (`run.sh`)

`run.sh` is the entry point. It rebuilds `target/release/brain` when
`Cargo.toml` or any `src/**/*.rs` is newer than the binary (build chatter to
stderr), then `exec`s the binary, forwarding every argument. It does **not**
capture stdout, parse a plan, or apply any parent-shell effect — the binary
handles its own effects.

The binary's **stdout** carries only:

- `brain config …` output (the config table, or a single value), and
- clap's help / version / error text.

Everything else is a TUI that renders to `/dev/tty`, so nothing an interactive
session paints reaches stdout. Diagnostics go to stderr.

## The tasks view (in-process, no handoff)

The tasks CSVs (`~/brain/tasks/{tasks,habits}.csv`) are read directly by
`brain`'s tasks main view (`crate::tasks`), and `brain tasks …` launches the
merged shell (or runs a tasks utility) in-process. The tasks-view side effects
that *do* shell out live in the tasks modules:

- **`~/global-skills/todo/scripts/mark_done.py`** — `brain tasks complete <id>`
  and the palette's mark-complete action `exec`/invoke it to mutate the CSVs.
- **`agenda` / `habits` zsh functions** — `Ctrl+A` (agenda) and the palette's
  "Open habits page" run these via the injected `ShellRunner`.
- **`cd <root> && <claude_cmd> …`** — the brain panel's PTY, shared by both
  main views (see below).

This is the "central dispatch" design: `brain` is the single terminal command,
and each capability is either an in-process main view (tasks, brain-directory
search) or a spawned process it drives (claude for conversational work,
Finder/editor for files, `markdown-to-pdf` for conversions).

## The brain panel: claude session + SessionStart hook + state DB

The persistent shell's brain panel spawns `claude` itself, inside a PTY
(`pty_pane.rs`), running `cd <root> && <claude_cmd> --resume <id>` or
`--session-id <id>` (`session::build_claude_command`). `<claude_cmd>` is the
configurable launch command (`config::Config::claude_command`, config variable
`claude_cmd`, default `claude --dangerously-skip-permissions`), spliced in
verbatim so it may carry its own flags; brain always appends the `--resume` /
`--session-id` flag it controls, so it never depends on a shell alias. The
PTY's working directory is **also** set to `<brain_root>` (resolved via
`paths::brain_root()`, honoring the `root` config), so claude resolves its
project dir (and the SessionStart hook in `.claude/settings.json`) under the
brain root from the first instant — every brain session is scoped there, so
resume always looks in the same place.

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

The search view opens files from inside the running TUI, via `open_target`'s
impure spawners; the brain panel never closes. A text file →
`open_in_editor_tab`, which runs `osascript` to open a **new iTerm2 tab**
(`iterm_new_tab_applescript` over `edit_shell_command` = `cd <dir> &&
${VISUAL:-${EDITOR:-nvim}} <file>`); a blob or directory → `open_with_system`
(`open <path>`); a Finder reveal (`Ctrl-Enter`) resolves to the parent dir
(`open_target::finder_target`) and calls `open` on it. Whether a file is text
or a blob is decided by `open_target::is_textlike`. On a non-iTerm2 terminal
the editor path falls back to `open <file>`. Nothing is emitted to stdout; the
shell stays up throughout.

## Handoff: `markdown-to-pdf` (the "Create PDF" command)

The "Create PDF" command (palette row / `Ctrl-G` on a `.md` file) converts the
highlighted markdown to a colocated same-name PDF and opens it. It reuses the
user's existing converter rather than reimplementing PDF generation.

`markdown-to-pdf` is a hard prerequisite. Its path is the config variable
`markdown_to_pdf_path`, auto-discovered on first run and validated at startup
(see [config.md](config.md) and `settings/`); a missing/invalid path fails
fast with a red error. `open_target::create_pdf` spawns that command directly
(`<file.md> --out <file.pdf>`) — invoking the command, not any shell-function
wrapper, since a child process can't call a shell function. The output path is
`open_target::pdf_output_path` (same directory, same stem, `.pdf`).

- **Same-name guarantee.** The converter's non-interactive mode does *not*
  overwrite an existing PDF — it writes a `-vN` variant. To keep the output
  name identical to the source, `create_pdf` removes any pre-existing PDF at
  the target path first, so the converter always writes the exact name.
- **Opening the result.** The conversion runs in place and the PDF is handed
  to `open_target::open_with_system` (`open <pdf>`) — the brain shell stays up.
- **Best-effort.** A converter failure is swallowed (like a failed file-open)
  so a broken toolchain can't tear the shell down.

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
- **Refresh after.** The search view re-walks its scope (`App::refresh`) and
  drops the trashed path (`picker::App::drop_path`), so the entry disappears
  from the list.
- **Best-effort.** A failed `osascript` is swallowed (like the PDF path) so a
  denied automation permission can't tear the shell down.

## The auto-rebuild

`run.sh` rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then `exec`s it. This is the only
reason an agent's source edit "takes effect" without a manual
`cargo build` — but you should still build/test explicitly while
developing (see [testing.md](testing.md)).
