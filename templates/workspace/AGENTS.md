# Agent instructions for this brain workspace

This directory is a **brain workspace**: a personal knowledge base organized
with the PARA method (Projects, Areas, Resources, Archive), plus a task system
under `tasks/`. See [README.md](README.md) for the layout.

A machine can hold several workspaces (a personal one, a shared family one, …).
Everything in this file is scoped to *this* one. Brain exports `BRAIN_WORKSPACE`
and `BRAIN_ROOT` into the agent sessions it launches, so prefer those over
hardcoding a path; from a terminal, `brain -w <workspace>` selects one.

## Always use the `second-brain` skill

**Any request that touches this workspace — adding, moving, renaming, archiving,
retrieving, or reorganizing material — MUST be handled through the
`second-brain` skill.** Invoke it before doing the work, not after. That skill
encodes the decision rules (PARA bucket choice, naming conventions, README and
status requirements, retrieval procedure) that the rest of this file assumes you
are following.

If you are unsure whether a request counts as "working with the brain", default
to yes: anything that would read, write, move, or summarize files here qualifies.

## Terminology

When the user says **"the brain"** or **"my brain"**, they mean this knowledge
base. Treat any such reference as scoped to this workspace root, not to any
other workspace on the machine.

## Your purpose here

Your job in this directory is **knowledge management, not software
engineering**. You will be asked to:

1. **Maintain the structure.** Create, rename, move, and archive files and
   folders so the PARA layout stays clean.
2. **Update and edit notes.** Reorganize, summarize, or extend existing notes.
3. **Retrieve knowledge.** Search the workspace to answer questions — "what did
   I write about X?", "where are my notes on Y?", "summarize everything under
   `areas/health/`".

If a request would create code, infrastructure, or anything outside this
workspace, confirm before doing it.

## The task system

`tasks/` holds this workspace's tasks and habits:

- Use the **`todo`** skill to add, complete, defer, assign, and plan work.
- Use the **`triage`** skill for past-due cleanup and weekly review.
- **Never hand-edit `tasks/tasks.csv` or `tasks/habits.csv`.** They are merged
  across machines by row identity, and a manual edit that rewrites rows or
  reorders columns can lose another machine's work. Go through the skills or the
  `brain` CLI, which keep the id counters and schema consistent.

## How to work here

- **CLI-first.** The user explores this workspace with terminal tools. Optimize
  for grep-ability and predictable paths.
- **All names are lower-case and kebab-case.** Directories, files, attachments.
  No spaces, no camelCase, no underscores unless a tool requires it.
- **Plain text wins.** Prefer `.md` for prose and `.csv` / `.json` / `.jsonl`
  for structured data, so everything stays searchable with ordinary tools.
- **Archive, don't delete.** Move stale material into `archive/`, mirroring its
  original path. Delete only on explicit instruction.

## Couple markdown notes with their media

A markdown note that exists to discuss, summarize, or annotate a specific
non-markdown file (PDF, image, audio, video, dataset, …) **must live in the same
subdirectory as that file**. A flat folder of PDFs, images, and notes — with no
clue which note belongs to which file — is the failure mode this prevents.

The shape to enforce:

```
my-paper/
  my-paper.pdf
  my-paper-notes.md
```

Not this:

```
my-paper.pdf
my-paper-notes.md
some-other.pdf
some-other-notes.md
unrelated-image.png
```

### When to create the subdirectory

- **Adding a note for an existing standalone file.** If `my-paper.pdf` already
  sits in a folder and you are asked to add notes on it, create `my-paper/`,
  **move** the PDF into it, and place `my-paper-notes.md` alongside. Don't leave
  the PDF stranded at the parent level.
- **Adding a file that already has notes.** Same in reverse: create the
  subdirectory and colocate both.
- **Adding both at once.** Create the subdirectory from the start.

### Naming

- Name the subdirectory after the **subject**, not the file type (`my-paper/`,
  not `pdfs/` or `my-paper-files/`).
- Inside, name the markdown `<subject>-notes.md` (or another clear suffix like
  `-summary.md`, `-review.md`) so siblings can't be confused for each other.
- One subject per subdirectory.

### When *not* to couple

- Standalone notes that don't refer to a specific file stay as loose `.md` files
  in the topic folder.
- Standalone media with no notes can stay loose too; promote to a subdirectory
  only once a markdown gets attached.
- A single note surveying *many* sources doesn't pull them all into one
  subdirectory; it links to them where they live.

## When you rename or move a markdown file

Other notes may link to it. Before finishing the task:

1. Search the workspace for every reference — `rg -F '<old-filename>' .` (or
   `grep -rF`). Use the bare filename, not the full path, so you catch links
   written relative to different directories.
2. Update each reference to point at the new path.
3. Verify with a second pass that no stale references remain.

Do this even for "small" renames. Broken links rot the brain over time.

## Files brain owns

These are managed by the `brain` tool. Don't hand-edit, rename, or reorganize
them, and don't treat them as notes:

- `.config/workspace.json` — this workspace's portable identity.
- `.config/config.json`, `.config/personalization.json`, `.config/users.json` —
  portable settings, persona, and members. Change them with `brain config`,
  `brain persona`, and `brain user`.
- `tasks/SCHEMA.json` — the task schema, and the documentation of every CSV
  column. Read it to understand the columns; don't edit it by hand.
- `tasks/.tasks_next_id`, `tasks/.habits_next_id` — id counters.
- `RCLONE_TEST` — the sync safety marker. Deleting it makes sync refuse to run
  until repaired.
- `.claude/`, `.codex/`, `.opencode/` — lifecycle bridges brain installs so an
  agent session can report back. Brain rewrites them as needed.

## This workspace may sync to other machines

If cloud sync is configured (`brain sync`), everything here travels to the
user's other machines. Two consequences worth internalizing:

- **Anything you create lands everywhere.** Keep machine-local build artifacts
  out of the workspace: dependency trees (`node_modules/`), virtualenvs, caches,
  editor state, downloaded binaries. They are per-machine by nature, they bloat
  the remote, and they slow every future sync. If a tool insists on generating
  them here, say so rather than committing to sync them.
- **A rename propagates.** That is fine and expected, but fix inbound links
  first (above), because every machine inherits the broken ones too.

## Extending brain

- **Your own skill:** put it at `.config/plugins/<name>/SKILL.md` and run
  `brain skills sync`. It installs into the shared agent registry and becomes
  available to every supported frontend.
- **Adjusting a bundled skill** (`second-brain`, `todo`, `triage`, `contacts`,
  `brain-knowledge-capture`, `article-summarizer`) without forking it: add your
  additions to `.config/extensions/<skill>.md`, then `brain skills sync`.

## Things to avoid

- **Don't invent new top-level directories** alongside the four PARA folders.
  The sanctioned non-PARA entries are `tasks/` and the dot-directories brain
  owns. If something doesn't fit, ask.
- **Don't rename or move user-authored files without confirming.**
- **Don't add tooling or build files at the workspace root** (`package.json`,
  `.venv`, `node_modules/`, lockfiles). Code that *is* a skill belongs in
  `.config/plugins/<name>/`.
- **Don't delete to tidy up.** Archive instead.
