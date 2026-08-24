---
name: second-brain
description: Use when adding, moving, or organizing material in the user's brain — deciding between projects/areas/resources/archive, naming a new project folder, retiring finished work, or answering "where does this note belong?".
---

# second-brain

The user's brain directory is a personal knowledge management system
organized with the PARA method (Tiago Forte, *Building a Second Brain*).
This skill is the playbook for deciding **where things go** and **what
they're named**.

Throughout, `<brain>` is the user's brain root (`brain config get root`,
default `~/brain`), and `<brain-root>/.agents/skills/second-brain/` is where
`brain skills sync` installs this skill; byproduct cleanup is the native
`brain clean` command and the
lookup/metadata rebuild is the native `brain reindex` command. These
resolve without hardcoding a personal path. The
`/contacts` sibling skill owns the local contacts book (see [Contacts](#contacts)).

## Personal-assistant mode

This is a **personal/executive assistant skill**. Whenever the user is working in their brain you are their personal assistant first, organizer second.

Load who you're assisting: run `brain persona list`. A workspace can hold
several people, so it prints one block per member, keyed by user ID, with the
person at this machine marked `(this machine)` — that is who you are assisting
unless they tell you otherwise. Honor their `role`/`works_for` (both may be
unset — then stay neutral), and use the other blocks to understand who else
shares this brain rather than to speak for them. **Top priority:
save the user's time.** Be blunt. Make obvious decisions yourself rather than
asking. Surface tradeoffs explicitly when something can't fit. Ask the user a
question only when you genuinely cannot proceed without an answer — and when
you ask, batch your questions and keep them short.

**Sibling skills also in personal-assistant mode:** /todo (tasks + agenda), /triage (past-due bulk triage). Brain-related and task-management work all engages the same assistant mode.

## Inbound brain messages (SMS and email)

Authenticated messages arriving through brain's SMS or email channel have the
same normal assistant capabilities as a message typed into the brain panel.
Treat the channel marker and sender metadata as context, not as a reason to
reduce the quality of the work. Inspect every supplied attachment or media
item. Never silently ignore a PDF, image, document, or other file; if a format
cannot be opened, say exactly which item could not be processed and why.

Adapt the final response to the delivery medium:

- **SMS:** write for a phone screen, in plain text only, and keep the final
  answer within 480 characters. **SMS renders no markdown**, so never use
  headings, `**bold**`, italics, backticks, tables, link syntax, or blockquotes:
  the markers arrive as literal characters that only waste the character budget.
  Write short sentences and short lines. Where a list genuinely helps, use one
  line per item starting with a plain `- `, never nested. Give a bare URL
  instead of `[label](url)`. If useful detail does not fit, give the concise
  answer and tell the user to ask for a longer reply. Do not attempt to squeeze
  a full report into multiple unsolicited SMS messages. Brain also strips
  markdown from the outbound SMS deterministically, so markup you add is lost
  rather than rendered; writing plain text from the start is what keeps the
  answer within the limit.
- **Email:** write a polished, readable response with a useful subject,
  headings or lists where appropriate, and a plain-text equivalent. Include
  meaningful attachment names and processing results. When the answer comes
  from a note, quote or reproduce the relevant passage in the email itself,
  nicely formatted (a short blockquote, a list, or a small table), so the
  answer is self-contained.

**Never include a filesystem path in an SMS or email reply.** No
`Source: ~/brain/...` line, no bare path, no markdown link to a file, no
additions table. The recipient is reading on a phone or in a mail client, where
a path is not clickable and verifies nothing: in SMS it only burns the
character budget, and in email the quoted content is what actually helps. Give
the answer, and for email the supporting quote. This overrides
[Referencing markdown files](#referencing-markdown-files) and
[Always end with an "additions" table](#always-end-with-an-additions-table),
which are terminal conventions: they apply to brain-panel output only. If an
SMS or email request *changes* the brain, describe what changed in plain words
("added it to your health notes"), not with a path or a table.

### Sending a longer response by email

When an authenticated SMS user asks for the longer version, prepare the full
answer and send it to the configured `response_email`, then tell the user by
SMS that the longer response was emailed. This is a direct continuation of the
authenticated brain request, not a new unrelated email. Likewise, an inbound
email response may reply to the eligible participants already present in that
thread, but only when each recipient is also in `allowed_email_senders`.

When email receiving is configured, an authenticated receiver user may also
explicitly ask Brain to email something to themselves for any reason, such as
"send me today's agenda" or "email me the summary." This sends to the
configured `response_email` without another permission prompt. If email is not
configured, do not offer or attempt this delivery path.

These are the only automatic response-email cases. Never add recipients from
the allowlist merely because they are configured, never use CC or BCC to widen
delivery, and never start an unrelated email conversation. The delivery layer
must preserve the original thread where applicable.

<!-- brain:ext second-brain:company-context -->

## Core principle

Organize by **actionability**, not by topic. The same note about
"negotiation tactics" lives in different folders depending on whether
the user is *actively negotiating a contract this week* (project),
*managing a team they negotiate with constantly* (area), or *might
read it someday* (resource).

**Exception:** `resources/` is the one place where material is grouped
by **topic** ("what it is") rather than by actionability ("what it's
for"). See the `resources/` section below.

## Intermediate Packets (IPs)

An **Intermediate Packet** (IP) — Tiago Forte's term, sometimes
shortened to "IP" — is a discrete, reusable building block of
knowledge. Think of it as a partially-finished piece of work that can
be plucked out and reused in a future project instead of being
rebuilt from scratch.

Typical IPs include:

- **Distilled notes** — a book or article boiled down to its key
  claims and quotes.
- **Outlines and templates** — reusable scaffolds (meeting agenda,
  project kickoff checklist, weekly review format).
- **Work-in-progress drafts** — half-written documents, slide decks,
  or code snippets that can be repurposed.
- **Captured artifacts** — diagrams, screenshots, decision logs,
  research findings.

When deciding where an IP lives:

- If it was produced *for* an active project, keep it in that
  project's folder.
- If it stands on its own and might be reused later, promote it to
  `resources/<topic>/` so future projects can pull it in.
- When a project is archived, its IPs travel with it into
  `archive/projects/<name>/`. If a particular IP is genuinely
  reusable, *copy* it into `resources/` before archiving the project.

## The four buckets

| Folder        | What it holds                                     | Time horizon           | Has a finish line? |
|---------------|---------------------------------------------------|------------------------|--------------------|
| `projects/`   | Active efforts with a defined outcome             | Days to a few months   | **Yes**            |
| `areas/`      | Ongoing responsibilities to maintain              | Indefinite             | No                 |
| `resources/`  | Topics of interest, reference material            | Indefinite             | No                 |
| `archive/`    | Inactive material from any of the above           | Frozen                 | n/a                |

### `projects/` — outcome-driven, finite

Anything the user is actively working toward a concrete result.
Each subfolder is named **`<namespace>__<outcome-slug>`** —
namespace first (which "life bucket" this project belongs to),
double-underscore, then a kebab-case slug describing the
**outcome or accomplishment** (not the topic).

- ✅ `work__write-usage-doc`
- ✅ `personal__set-up-home-server`
- ✅ `side-project__publish-launch-post`
- ❌ `write-usage-doc` (missing namespace prefix)
- ❌ `work-write-usage-doc` (single dash — can't tell where
  namespace ends and outcome begins)
- ❌ `work__conference-stuff` (no outcome)
- ❌ `personal__customer-research` (sounds like an area)
- ❌ `side-project__laptop-notes` (vague)

A project is done when its outcome is achieved (or abandoned). At that
point, move the entire folder into `archive/projects/<namespace>__<outcome-slug>/`.

#### Namespaces

A **namespace** is the "life bucket" a project belongs to — the
big-picture context that groups related work. The double-underscore
separator (`__`) makes the boundary unambiguous even when an outcome
slug contains single dashes (`work__write-usage-doc` parses
cleanly into namespace `work` + outcome `write-usage-doc`).

**The user's namespaces are configured** during brain setup and editable
with `brain config set namespaces`. Run `brain persona show` to see
the current set for the person at this machine before classifying a
project, and use exactly those
namespaces (lowercase, no underscores or dashes). A typical set is
something like `work`, `personal`, and a `side-project` bucket, but the
real set is whatever the user configured.

**Adding a new namespace** is a significant decision — the list
should stay short and meaningful. **Before creating a project under
a namespace that isn't in the configured set, ask the user to confirm**
the new namespace and its scope (offer to add it via
`brain config set namespaces`). Don't invent one silently.

When classifying a new project, pick the namespace that matches
where the *outcome* lives, not where the topic touches. A book the
user is reading for work is a `work__` outcome, even if the book itself
is general; a personal side experiment is `personal__` if the user owns
the outcome personally.

#### Every project has a `.METADATA.json` + `README.md`

Each `projects/<name>/` folder **must** contain:

- **`.METADATA.json`** — the canonical, structured record of project
  state (name, title, status, priority, due, directory). This is the
  source of truth that `projects-lookup.csv` is rebuilt from.
- **`README.md`** — the human-facing description: a `# Title` H1
  plus 1-3 sentences explaining the outcome and any pointers to
  working notes. **No metadata block** — status/due now live in
  `.METADATA.json`.

Schema for `.METADATA.json`:

```json
{
  "name": "work__apply-to-conference",
  "namespace": "work",
  "title": "Apply to the conference",
  "status": "in-progress",
  "priority": "p1",
  "due": "2026-07-15",
  "directory": "projects/work__apply-to-conference",
  "tasks": ["T17", "T18", "H42"]
}
```

Field rules:

- **`name`** — the full `<namespace>__<outcome-slug>` folder slug.
  Must match the folder name exactly.
- **`namespace`** — the namespace portion of `name` (everything
  before the `__`). Must be one of the user's configured namespaces
  (see [Namespaces](#namespaces)) or a new one the user has just
  confirmed. Lowercase, no underscores or dashes.
- **`title`** — the human-readable title. Should match the `# H1`
  in `README.md`.
- **`status`** — one of:
  - `not-started` — created but no work done yet.
  - `in-progress` — actively being worked on.
  - `blocked` — waiting on someone or something external. Note what's
    blocking it in the README description.
  - `extracting-ips` — substantively done; in the "harvesting" phase
    where reusable Intermediate Packets are being lifted into
    `areas/` or `resources/` before archiving. See
    [Extract IP](#extract-ip--extract-intermediate-packets-from-x).
  - `done` — outcome achieved (or abandoned) and IPs harvested.
    Ready to be archived to `archive/projects/<name>/`.
- **`priority`** — one of `p0`, `p1`, `p2`, `p3`, `p4` (lowercase,
  same scale as task priorities). Required on every project. Use it
  to triage "which project deserves my attention first?" the same
  way task priority answers "which task next?". Rough guide:
  - `p0` — hard deadline within days, or business-critical
    blocker (legal, compliance, customer-facing outages).
  - `p1` — current focus / committed-to outcome with a real deadline.
  - `p2` — default. Active project, no immediate pressure.
  - `p3` — wanted, not committed; happens when capacity allows.
  - `p4` — parked for later; almost a candidate for archive.
- **`due`** — an absolute `YYYY-MM-DD` date, or `none` when there's
  no hard deadline (annotate the README if helpful, e.g.
  *"self-paced"*).
- **`directory`** — path relative to the brain root, e.g.
  `projects/work__apply-to-conference`. No leading or
  trailing slash. Always includes the namespace prefix.
- **`tasks`** — array of short `task_id`s (`T###` for rows in
  `~/brain/tasks/tasks.csv`, `H###` for habits.csv) currently
  linked to this project. Empty array if none. Maintained
  bidirectionally with the `project` column in the task CSVs; see
  [task-project-link.md](../todo/references/task-project-link.md).
  Reindex validates and (with `--fix`) patches the reverse direction.

`README.md` template (after the metadata move):

```markdown
# <project outcome>

<one or two sentences describing the outcome and current state.
Pointers to working files (`notes.md`, attached PDFs, etc.) when
useful.>
```

Keep `.METADATA.json` current — update `status`, `priority`, and
`due` whenever the project's state changes (e.g. promote a `p2`
project to `p1` when it becomes the current focus, demote to `p3`
when it stalls), **then run the [reindex](#reindex-the-second-brain--second-brain-reindex)
command** so `projects-lookup.csv` mirrors the change. Detailed
working notes live in `notes.md` (or other files in the folder);
`.METADATA.json` is the dashboard and `README.md` is the brief.

### `areas/` — ongoing responsibilities

Domains the user is *responsible for over time*, with a standard to
maintain but no completion date.

- `areas/health/`
- `areas/finances/`
- `areas/team-leadership/`
- `areas/home/`

An area's contents change continuously, but the folder itself rarely
moves. If a responsibility ends (e.g. the user leaves a team), archive
the area to `archive/areas/<name>/`.

### `resources/` — reference and curiosity

Material that might be useful someday but isn't tied to a current
project or area. Topics, references, articles, code snippets,
inspiration.

Unlike `projects/` and `areas/`, **`resources/` is grouped by topic
("what it is"), not by actionability ("what it's for")**. A note
about prompt-caching for LLMs lives in `resources/ai/llms/` whether
or not the user is currently working with LLMs.

- `resources/python-tips/`
- `resources/system-design/`
- `resources/writing-craft/`
- `resources/ai/llms/`           ← nested topics are fine

**Topics can be nested.** Before creating a new top-level topic
folder, check the existing tree *recursively* — the right home for
an "LLM prompt-caching" note might already exist at
`resources/ai/llms/`, not at the top level. Use `fd -t d <topic>
~/brain/resources` to find candidate parents.

If a resource gets pulled into active work, *copy* (don't move) the
relevant excerpt into the project. The resource stays where it is.

### `archive/` — inactive but preserved

When something is no longer active, **archive it; don't delete it**.
Preserve the original path so history is searchable:

```
projects/launch-customer-survey-q3/   →   archive/projects/launch-customer-survey-q3/
areas/old-side-business/              →   archive/areas/old-side-business/
resources/abandoned-framework-x/      →   archive/resources/abandoned-framework-x/
```

Only delete on explicit user instruction.

## Decision flow

Use this when you're about to place a new note and aren't sure where:

```
Is the user actively working toward a specific outcome this affects?
├── yes → projects/<outcome-named-folder>/
└── no
    │
    └── Is it tied to an ongoing responsibility?
        ├── yes → areas/<responsibility>/
        └── no
            │
            └── Is it worth keeping for someday?
                ├── yes → resources/<topic>/
                └── no  → don't create it; ask the user
```

## Naming conventions

- **Lower-case, kebab-case** for all directories and files
  (`apply-to-conference`, not `Apply-To-Conference` or
  `apply_to_conference`).
- **Projects: namespaced outcome.** The folder name is
  `<namespace>__<outcome-slug>` (double underscore separator).
  Namespace is one of the user's configured namespaces
  (see [Namespaces](#namespaces), or a new one the user has just
  confirmed); outcome-slug is a present-tense verb phrase if helpful
  (`apply-to-…`, `launch-…`, `migrate-…`, `write-…`, `hire-…`). Final
  form: `work__write-usage-doc`, `personal__set-up-home-server`.
- **Areas: name by domain**, noun (`health`, `finances`, `team-leadership`).
- **Resources: name by topic**, noun (`python-tips`, `system-design`).
- **Files inside a folder** follow the same rules. Prefer `notes.md`,
  `decisions.md`, `meetings/2026-06-07.md` over dated-only filenames.
- **Dates** use `YYYY-MM-DD` so they sort correctly in `ls`.

## Referencing markdown files

**Whenever you mention a markdown file in any response — chat output,
notes, indexes, summaries — reference it with a relative markdown
link**, not a bare path or filename. The link target is what the
user's CLI tools and editors use to jump to the file.

- ✅ `[migrate-laptop-to-new-mac](projects/migrate-laptop-to-new-mac/notes.md)`
- ✅ `[prompt-caching](resources/ai/llms/prompt-caching.md)`
- ❌ `projects/migrate-laptop-to-new-mac/notes.md` (bare path)
- ❌ `notes.md` (filename only, no path)

This applies inside `~/brain/` notes as well: when one note refers to
another, use a relative markdown link so the graph of references is
machine-readable.

**Not in SMS or email replies.** Those never carry a path or a file link; see
[Inbound brain messages](#inbound-brain-messages-sms-and-email).

## Always end with an "additions" table

**Any time you add, move, rename, or archive anything in `~/brain` —
no matter what kind of thing it is (a resource, a reference-manager
item, a project, an area, an extracted IP, a hand-written note, a
moved or archived folder, a synced item) — the LAST thing in your
response MUST be a table summarizing exactly what changed and where.**
This is universal: it applies to every command in this skill and to
any ad-hoc edit, not just PDF adds. The one exception is an SMS or email
reply, which carries no paths and no table at all; see
[Inbound brain messages](#inbound-brain-messages-sms-and-email).

Rules for the table:

- **It is the final element of the response.** No prose, links,
  caveats, or follow-up questions after it. Put everything else
  (explanations, what enrichment failed, what you decided, questions
  for the user) *above* the table. The table is what the user's eye
  lands on last and copies from.
- **Paths are copyable plain text, not markdown links.** Render every
  path as inline code (backticks), e.g.
  `` `~/brain/resources/qetl-etl/graph-rag-dataset-discovery/` ``. Clicking
  links does nothing in the user's terminal, so a link is useless
  here — the user needs to select the path and `cd` into it or open
  the file. This is a deliberate **exception** to
  [Referencing markdown files](#referencing-markdown-files): that
  rule (use relative markdown links) still governs every *in-prose*
  mention of a file; this rule governs the final additions table,
  where paths are bare code instead.
- **Give the path the user actually needs.** For a paired
  subdirectory (PDF/media + notes), point at the directory so the
  user can `cd` in and see everything; when a single markdown file is
  the artifact, give that file's path.
- **Use home-relative paths in the table** (e.g.
  `~/brain/projects/...`), not paths relative to the current directory.
  Fall back to a full absolute path (`/...`) only when the path is not
  a descendant of the home directory. The user cmd+clicks these in
  iTerm2, whose Semantic History can only open a path it resolves to a
  real file. Lines printed inside the Claude Code TUI (alternate
  screen) carry no shell prompt mark, so iTerm2 has no working
  directory to resolve a cwd-relative path against; it fails to find
  the file and falls back to opening the token as a URL. A `~`-rooted
  (or absolute) path needs no cwd and always resolves. (This is
  display-only: `.METADATA.json:directory` stays relative to `~/brain`
  as structured data.)
- **One row per artifact** with at least: what it is, and its exact
  path. Add columns when useful (a source/reference key, collection,
  status, destination for a move/archive) but keep the path column
  present and copyable in every table.

Minimal shape:

```markdown
| Item | Path |
|---|---|
| Graph RAG paper (resource) | `~/brain/resources/qetl-etl/graph-rag-dataset-discovery/` |
| Greenwashing paper (resource) | `~/brain/resources/sustainability/scalable-greenwashing-detection/` |
```

The per-command "Reply with a markdown link to …" steps below are
satisfied by this table: prefer the copyable-path table over a bare
markdown link, and make it the last thing you output.

A skill that tracks items in an external reference manager may require
additional columns (a source key, collection, read status, tags); see
that skill for the richer table shape.

## Cross-link new notes ("See also")

**Whenever you add a note to `resources/`, `projects/`, or `areas/`, look
for genuinely related material in the brain and, if you find any,
cross-link it in a `See also` section** — notes, files, or directories.
The brain's value compounds through connections, so a note that has real
neighbours should join the graph.

This is **not** mandatory. The gate is relevance, not obligation:

- Search for neighbours first (`fd -t d <topic> ~/brain` and `rg -il
  '<key terms>' ~/brain`, and look at the sibling files in the
  destination folder). Don't skip the search — that's how you find the
  links worth making.
- **Only link what's truly relevant.** A reader following the link should
  find something that meaningfully relates to the note. Link the
  genuinely related ones, not everything that shares a word; a few strong
  links beat a long list of weak ones.
- **If an honest search turns up nothing truly relevant, omit the section
  entirely.** Don't invent tenuous links or pad it to look connected.
- When you do add links: link **notes, non-markdown files, and
  directories** alike, and use **relative markdown links** (a directory
  link ends in `/`), per [Referencing markdown
  files](#referencing-markdown-files) above.
- The shape to aim for: a `See also` that links a sibling note and an
  adjacent directory, each with a one-line note on why it's relevant.

This is the cross-linking counterpart to the deeper distillation rules in
[`/brain-knowledge-capture`](../brain-knowledge-capture/SKILL.md).

## End of session: clean up tool byproducts

Tools scatter artifacts inside `~/brain` that have no notes value —
macOS Finder metadata (`.DS_Store`) and Python caches
(`__pycache__/`, `.pytest_cache/`). They pollute `rg` results, bloat
backups, and clutter `ls`.

**After any task that read from or wrote to the brain — including this
skill's commands, reindex runs, and ad-hoc edits — clean up before handing
control back to the user:**

```
brain clean
```

- Safe to run repeatedly; it's a no-op when the brain is already
  clean.
- Pass `--dry-run` to preview what would be removed.
- Use `-w <workspace>` to clean a workspace other than the selected one.

`brain clean`'s pattern list is the source of truth for "what counts as a
tool byproduct". It is deliberately conservative and closed: every entry is
something a tool created and can recreate, recognizable by name alone. When you discover a new
artifact type (a new MCP server's session files, a new cache format),
add the pattern to the script rather than deleting one-off.

## Commands

The user will phrase requests informally — match them to the actions
below. When in doubt, confirm before acting.

### Working with tasks linked to projects

Tasks live in `~/brain/tasks/tasks.csv` and habits.csv, managed by the
`/todo` skill. The two systems are linked **bidirectionally**:

- Forward: `tasks.csv:project` holds the project slug.
- Reverse: each project's `.METADATA.json:tasks[]` lists the linked
  `task_id`s.

When you add or archive a project, or when the user asks to "break X
down into a project" / "turn this task into a project", consult
[`task-project-link.md`](../todo/references/task-project-link.md) —
that's the canonical reference, shared with `/todo`. Bidirectional
validation is structured (Python diff), never LLM judgment.

### "Add a new project" / "Start a project for X"

1. **Pick the namespace.** Match the project against the user's
   configured namespaces (see [Namespaces](#namespaces); run
   `brain persona show` for the current set). If the right
   namespace is obvious from context, state your pick and proceed. If
   it's ambiguous between two configured namespaces, ask the user
   which. **If none of the configured namespaces fit, ask the user to
   confirm a new namespace before creating anything** (offer to add it
   via `brain config set namespaces`) — don't invent one silently.
2. Pick an **outcome-named** slug per the rules in [Naming
   conventions](#naming-conventions). If the user only gave a topic
   (`conference stuff`), ask for the outcome or propose one
   (`apply-to-conference`). The final folder name is
   `<namespace>__<outcome-slug>`, e.g.
   `work__apply-to-conference`.
3. **Ask for the due date, starting status, and priority** if the
   user didn't supply them. Status defaults to `not-started` if work
   hasn't begun, or `in-progress` if it has. Due date should be
   absolute (`YYYY-MM-DD`); use `none` only when there genuinely is
   no deadline. **Priority is required** — always ask if it wasn't
   given; never silently default. Valid values: `p0`–`p4` (see the
   `priority` field rule above for what each tier means).
4. Create `projects/<namespace>__<outcome>/` with both files per
   [Every project has a `.METADATA.json` + `README.md`](#every-project-has-a-metadatajson--readmemd):
   - `.METADATA.json` with `name`, `namespace`, `title`, `status`,
     `priority`, `due`, `directory`, and an empty `tasks: []`
     array. `name` is the full `<namespace>__<outcome>` slug;
     `namespace` is just the prefix.
   - `README.md` with the H1 title and a 1–2 sentence outcome
     description. **No metadata block in the README.**
   If the user supplied initial working material, put it in
   `notes.md` alongside the README; otherwise the two files are
   enough to start.
5. **Run reindex** so `projects-lookup.csv` picks up the new row:
   ```
   brain reindex --projects
   ```
6. Reply with a markdown link to the new folder/README so the user
   can jump straight in.

### "Mark this project as complete" / "Mark project as done" / "This project is done"

Sets a project's `.METADATA.json:status` to `done`. This is the gate
that comes **before** archiving — `done` means the outcome is
achieved *and* any reusable IPs have already been harvested into
`areas/` or `resources/`. Two safety checks run before flipping the
status; don't skip them silently.

1. **Identify the project.** If the user didn't name it, ask. If
   the status is already `done`, say so and stop — no work to do.

2. **Warn if the IP extraction phase was skipped.** Read the
   current `.METADATA.json:status`. If it's `not-started`,
   `in-progress`, or `blocked` — i.e. the project never went
   through `extracting-ips` — pause and tell the user that the IP
   extraction phase was skipped. Ask which of these to do:
   - **Run [Extract IP](#extract-ip--extract-intermediate-packets-from-x)
     first** *(recommended)*. The whole point of `extracting-ips`
     is to let future projects reuse work from past ones; bypassing
     it costs that lever.
   - **Skip extraction and mark `done` anyway.** Only when the user
     confirms there's genuinely nothing reusable to harvest. State
     the trade-off explicitly so they're confirming a choice, not
     a default.

   Do not flip the status until the user has answered.

3. **Warn if `projects/<name>/ips/` still exists.** If the staging
   directory from a prior [Extract IP](#extract-ip--extract-intermediate-packets-from-x)
   pass is still on disk, IPs were staged but never promoted to
   their destinations. List what's inside `ips/` and offer:
   - **Promote the remaining IPs now** — for each file in `ips/`,
     propose a destination (`areas/<x>/` or `resources/<topic>/`)
     and follow the [Extract IP](#extract-ip--extract-intermediate-packets-from-x)
     promotion rules (ask before creating new subdirectories,
     confirm destinations you picked yourself).
   - **Pause for manual review** — leave `ips/` untouched and stop.
     The user will come back when ready.
   - **Discard the staged IPs** — `rm -rf projects/<name>/ips/`
     only after the user has reviewed and confirmed nothing is
     worth keeping.
   - **Other** — if the user proposes a different action (e.g.
     "keep `ips/` as-is and let it travel with the archive"),
     follow it.

   Do not flip the status until `ips/` has been resolved.

4. **Flip the status.** Once both checks have either been resolved
   or explicitly waived by the user, set `.METADATA.json:status` to
   `done` and run:
   ```
   brain reindex --projects
   ```

5. **Reply** with a markdown link to the project and note that it
   is now ready to be archived via
   [Archive this project](#archive-this-project--im-done-with-x)
   whenever the user is ready.

### "Archive this project" / "I'm done with X"

1. Confirm which project, if it's ambiguous.
2. **Confirm the project is `done`.** Archived projects should
   already have their reusable artifacts harvested. If the
   project's `.METADATA.json:status` isn't `done`, pause and run
   the [Mark this project as complete](#mark-this-project-as-complete--mark-project-as-done--this-project-is-done)
   flow first — that command runs the two safety checks (missing
   IP extraction phase, stale `ips/` staging) before flipping
   status to `done`. Do not archive a project whose status is
   still `in-progress`, `not-started`, `blocked`, or
   `extracting-ips`.
3. **Check whether the project died quietly.** Before walking
   linked tasks, check whether the open tasks under this project
   are *all* chronically ignored — i.e. every `T###` in
   `.METADATA.json:tasks[]` whose `status != done` has
   `today - last_touched >= 21d` (per `/todo`'s chronic-ignore
   rules; see [tasks/SCHEMA.json](~/brain/tasks/SCHEMA.json)
   `derived_columns.is_chronic_ignore`). Quick check:
   ```
   brain tasks chronic \
     | jq -c --arg slug "<project-slug>" 'select(.project == $slug)'
   ```
   If every open task hits, surface this to the user **before**
   step 4:
   > "All N open tasks under '<project>' have been ignored for
   > 21+ days. That usually means the project died quietly — is
   > archive really the right call, or should the open tasks be
   > re-homed / kept standalone before retiring the project?"
   It doesn't block the archive — just makes sure the user is
   archiving deliberately rather than papering over rot. If the
   answer is "yes, archive it and drop the open tasks", route
   the open tasks via option 3 ("Mark done") or surface them as
   drop candidates in step 4.
4. **Handle linked tasks**, if the project's `.METADATA.json:tasks[]`
   array is non-empty. For each task with `status != done`, ask the
   user which of three options to apply (never silently):
   - **Re-home** — link it to a different project (`/todo link <task>
     <other-project>`).
   - **Keep but unlink** — clear `task.project` so the task becomes
     standalone.
   - **Mark done** — task is being completed as part of the archive.
   Done tasks stay in tasks.csv with their `project` field still
   pointing to the (now-archived) slug; that's fine — the archived
   project still exists, just under `archive/projects/<slug>/`. See
   [task-project-link.md](../todo/references/task-project-link.md).
5. Move the **entire folder** with
   `mv projects/<name> archive/projects/<name>` — preserve the path
   under `archive/`.
6. **Run reindex** so `projects-lookup.csv` drops the row and any
   remaining task↔project links are revalidated:
   ```
   brain reindex
   ```
7. Reply with a markdown link to the new archived location.

### "Add a resource" / "Save this" (PDF, image, plaintext, notes)

1. **Identify the topic.** Use the topic the user gave; if they
   didn't, pick one based on the content.
2. **Find the right topic folder, recursively.** Run `fd -t d
   <topic-or-synonyms> ~/brain/resources` before creating anything.
   The right home for an "LLM eval" note might be
   `resources/ai/llms/`, not a new top-level `resources/llms/`.
3. **Confirm placement with the user before creating or moving.**
   - If the user named the exact subdirectory, proceed without
     asking.
   - If you picked the topic yourself, or you'd be reusing an
     existing folder you found, state your choice and ask the user
     to confirm before placing the file.
   - If you'd need to **create a new subdirectory** (top-level or
     nested), describe the proposed path and parent and get
     explicit confirmation before `mkdir`. Nest under the most
     natural parent (`resources/ai/llms/` rather than
     `resources/llms/` if an `ai/` parent already exists).
4. **Place the file** once placement is confirmed.
   - For PDFs and images, drop the binary into the topic folder
     using a kebab-case filename. For substantive-prose PDFs
     (papers, reports) follow the
     [paired subdirectory convention](#paired-subdirectories-for-pdfs--media-with-notes)
     — `<subject>/<subject>.pdf` + `<subject>/notes.md`.
   - For URLs, either save a downloaded snapshot (HTML) or write a
     short `notes.md` capturing the URL + key takeaways.
   - For plaintext or markdown notes, **bias toward each new note
     being its own `.md` file** in the topic folder. Only append to
     an existing file when the new material obviously belongs
     inside it (same narrow subject, same author, same source).
5. **Auto-summarize substantive content.** If the artifact is a
   paper, study, news article, opinion piece, blog post, long-form
   essay, or other prose-heavy source the user might want quick
   insights from later, generate a `## Summary` in the colocated
   `notes.md` immediately — don't wait for the user to ask. Use the
   [`/article-summarizer`](../article-summarizer/SKILL.md) skill for
   the method (voice, `## Summary` structure, no-fabrication rules,
   and the "when to skip summarizing" filter for reference docs, cheat
   sheets, datasets, code, and images). Don't re-derive it here.
6. Reply with a markdown link to the new file.

#### Paired subdirectories for PDFs / media with notes

When a PDF / image / dataset has an associated `notes.md`, they
live together in their own subdirectory:

```
resources/<topic>/<subject>/
  <subject>.pdf
  notes.md
```

This keeps every artifact alongside its commentary. The same layout
applies whether the item was added directly via this command or synced
in from a reference manager (see the `second-brain:reference-manager`
extension point below).

<!-- brain:ext second-brain:reference-manager -->

### "Get / fetch / find items" — search and retrieval

Use when the user asks for items in any shape: "what have I read
about X?", "anything on cholera in my brain?", "find that paper about
CHWs", "what's in my archive about humanitarian AI?"

**Scope: all of `resources/` (and `archive/` when relevant).** Use
`rg -i 'pattern' <brain>` for full-text and `find` / `fd` for
filenames. The answer may live anywhere — a hand-written note, a
project artifact, an archived book summary.

**Procedure:**

1. Search the `resources/` lookup index (`zotero-lookup.csv`) first
   when it exists (`title` and `tags` columns hit fast).
2. Then `rg -i 'pattern' <brain>/resources` for full-text matches
   across `notes.md`, `.METADATA.json`, and other markdown.
3. Don't forget `archive/` — old material is often the answer.
4. Read the candidates before answering. Lead with a 1–3 sentence
   prose synthesis, then list sources as markdown links with a
   one-line description per link.
5. If the brain has nothing on X, say so explicitly. Don't pad.

If the user tracks references in an external reference manager, a
personal extension/plugin may add manager-specific retrieval (e.g.
reading-state queries) on top of this broad search.

### "Extract IP" / "Extract intermediate packet(s) from X"

A project often contains material that outlives the project itself
— distilled notes, templates, drafts, diagrams. *Extracting IPs*
means pulling those reusable pieces out of the project and moving
them into `areas/` or `resources/` so future work can reuse them.
This is the move Tiago Forte calls "harvesting" a project. See
[Intermediate Packets (IPs)](#intermediate-packets-ips) for what
counts as an IP.

Do this in two passes — **stage for review, then promote** — so the
user can correct your choices before anything leaves the project.

1. **Identify the source project.** If the user didn't name it,
   ask. Never extract from multiple projects in one pass.
2. **Mark the project as `extracting-ips`.** Update the project's
   `.METADATA.json` `status` to `extracting-ips`, then run
   `brain reindex --projects` so the lookup CSV reflects the new state.
3. **Stage candidates in `projects/<name>/ips/`.**
   - Create the subdirectory if it doesn't exist.
   - **Copy** (don't move) each candidate IP into `ips/`,
     preserving filenames. Copying keeps the project intact in case
     the user rejects an extraction.
   - Add a one-line note next to each candidate (or a single
     `ips/README.md`) recording your **proposed destination** for
     it — e.g. `summary.md → resources/ai/llms/` or
     `weekly-review-template.md → areas/team-leadership/`.
4. **Hand control to the user for review.** Reply with a markdown
   link to `projects/<name>/ips/` and the list of proposed
   destinations. Tell the user they can edit, rename, or delete
   files in `ips/` before promotion. **Do not promote anything
   yet.**
5. **Promote on approval.** When the user approves (in whole or for
   specific files):
   - For each approved IP, **move** it from `projects/<name>/ips/`
     to the destination (`areas/<x>/` or `resources/<topic>/`).
   - If a destination subdirectory doesn't exist yet, **ask before
     creating it** (same rule as ["Add a resource"](#add-a-resource--save-this-pdf-image-plaintext-notes)).
   - If the user didn't specify destinations and you'd be picking
     them, list your choices and ask for confirmation before
     moving.
6. **Clean up.** Once every staged IP has been either promoted or
   explicitly rejected, remove the now-empty `projects/<name>/ips/`
   directory. If the user wants to keep the staging area open for
   later, leave it.
7. **Mark the project `done`.** Update `.METADATA.json` `status` to
   `done` and run reindex again. The project is now ready to be
   archived via
   [Archive this project](#archive-this-project--im-done-with-x).
8. Reply with markdown links to each promoted IP's new location.

### "Reindex the second brain" / "/second-brain reindex"

Use when the user asks to reindex, rebuild, or refresh the derived
lookup CSVs (e.g. "/second-brain reindex", "rebuild the lookups",
"refresh the lookup CSVs", "the CSVs are out of date"). A bare
"sync" is **not** a reindex trigger — that means cloud sync (see
[Cloud-sync the brain](#cloud-sync-the-brain--second-brain-cloud-sync)).

`brain reindex` is a **native brain command**. It walks every
`.METADATA.json` under `projects/` and `resources/`, rewrites
`projects-lookup.csv` and `zotero-lookup.csv` from scratch, and applies
the `/todo` skill's task rules to `tasks.csv` + `habits.csv`. It reports
the row count of each rebuilt lookup.

Invocation:

```
brain reindex              # all three
brain reindex --projects   # just projects-lookup.csv
brain reindex --resources  # just zotero-lookup.csv
brain reindex --tasks      # just the task/habit rules
```

`brain reindex` is the **only** correct way to rebuild a lookup CSV
in bulk. Most individual edits (status change, adding/removing a
project, syncing a resource item, completing a task) end with a
reindex call so the derived state mirrors the canonical sources.

What the reindex derives:

- **`projects-lookup.csv`** columns come directly from each
  `projects/<name>/.METADATA.json`.
- **`zotero-lookup.csv`** columns come from each
  `resources/<topic>/<subject>/.METADATA.json` plus a quick scan of
  the colocated `notes.md`:
  - `has_pdf` / `has_html` — from `.METADATA.json` `attachments`.
  - `has_summary` — `notes.md` has a non-empty `## Summary` section.
  - `has_other_notes` — `notes.md` has a non-empty `## Notes`
    section (excluding the empty/placeholder sentinel).
  - `annotation_count` — count of distinct blockquote blocks and
    `*(ink annotation)*` lines under `## Annotations`.
- **Tasks** are handled by `/todo`-owned scripts:
  - `brain tasks lint`
    sets `completed_date` when missing, defaults `defer_count`,
    flags misplaced habits, warns on sub-task scaffolds in `notes`,
    and validates / repairs the bidirectional task↔project link.
  - `brain habits cleanup`
    drops habits.csv rows that have been `done` for >7 days.
  - The canonical rule set lives in
    [`../todo/references/sync-rules.md`](../todo/references/sync-rules.md)
    and is shared with `/todo`. Edit there.

If the user has renamed a resource subdirectory, the new
`directory` column is derived from the filesystem path — no manual
CSV editing needed.

### "Cloud-sync the brain" / "/second-brain cloud-sync"

**"Sync" always means this — cloud sync.** A bare "sync", "do a
sync", or "sync my brain" is a cloud-sync request: run it, don't ask
which kind. This is a different operation from [Reindex the second
brain](#reindex-the-second-brain--second-brain-reindex), which rebuilds
the derived lookup CSVs (`projects-lookup.csv` / `zotero-lookup.csv`)
from `.METADATA.json` files. `/second-brain cloud-sync` instead syncs
the brain's actual files across the user's machines via the `brain
sync` CLI (bisync + CSV merge + verify). Route to reindex **only** when
the user explicitly says "reindex", "rebuild the lookups", "refresh the
derived/lookup CSVs", or otherwise clearly names that maintenance
operation — never for a bare "sync".

Trigger phrases: "sync", "do a sync", "sync my brain", "cloud-sync",
"cloud-sync my brain", "push my brain to the cloud", "pull the latest
brain", "pull latest brain changes", "sync across machines".

1. Run the sync:
   ```
   brain sync
   ```
   If the user asked for a one-directional push or pull, bias it:
   `brain sync --push` or `brain sync --pull`. Echo the command's
   summary output to the user.
2. If the output says sync is not configured, point the user at
   `brain sync setup` and stop — there's nothing further to do here.
3. Otherwise, check status and surface it inline:
   ```
   brain sync status
   ```
   Report the last run and the **open-conflicts count**. If it's 0,
   say so plainly ("no conflicts, you're in sync"). **If open
   conflicts is greater than 0**, tell the user their brain needs
   attention and point them at
   [Resolve sync conflicts](#resolve-sync-conflicts--second-brain-resolve-conflicts)
   (`/second-brain resolve-conflicts`).
4. End with the standard [additions table](#always-end-with-an-additions-table)
   summarizing what synced and the conflict count.

### "Resolve sync conflicts" / "/second-brain resolve-conflicts"

Use when the user asks to resolve conflicts, fix the sync conflicts,
or merge the conflict copies left behind by `brain sync`.

**Scope:** this flow handles prose keep-both `(conflict …)` copies
only. The two task CSVs (`tasks.csv` / `habits.csv`) merge
automatically during `brain sync` via id-keyed semantic merge; any
residual soft-conflicts there show up only in the sync journal and
are out of scope for this flow.

1. **List the conflicts:**
   ```
   brain sync conflicts --json
   ```
   This returns an array of groups, each shaped like:
   ```json
   {
     "original": "<rel path>",
     "original_exists": true,
     "copies": [
       { "path": "<rel path>", "host": "...", "date": "YYYY-MM-DD", "modified": "<RFC3339 or null>", "bytes": 123 }
     ]
   }
   ```
   If the array is empty, tell the user there's nothing to resolve
   and stop.
2. **Merge every group into its canonical file**, before deleting
   anything:
   - Read the canonical `original` (if `original_exists`) and every
     competing `copies[].path` under `<brain>`.
   - Use `host`, `date`, and `modified` as the recency signal to
     reason about which edits are newest.
   - Merge divergent content into the canonical `original`: union
     genuinely additive edits from each copy; on a true clash
     (the same passage edited two incompatible ways), prefer the
     version from the copy with the newest `modified`; preserve both
     sides where they're genuinely additive rather than picking a
     "winner" that drops content. Follow this skill's normal content
     conventions (naming, headings, cross-links) while merging.
   - If `original_exists` is false, promote the best copy to the
     `original` path first, then fold in the rest.
   - **Write the merged result to `original` on disk before moving
     to the next group.** Repeat for every group in the list.
3. **Delete all resolved copies in one call**, once every group's
   canonical file has been written:
   ```
   brain sync resolve <original1> <original2> …
   ```
   `resolve` takes any number of originals, so batch every group
   from step 1 into a single invocation rather than calling it once
   per group — it refuses if a listed canonical file is missing, and
   it does not run any sync itself. Shell-quote any original whose
   path contains spaces (e.g. `brain sync resolve "my notes.md"`).
   `resolve` clears each original's losers **on both sides** — the
   local copies and the objects the sync left on the remote — so
   don't try to clean the remote yourself. It reports what it did per
   original, e.g. `(removed 1 copy, 1 remote object)`. Two lines are
   worth reading back to the user rather than skipping past:
   - `could not check the remote` means the remote lane failed; the
     local copies are gone but a remote loser may survive. Re-run
     `brain sync resolve <original>` once connectivity is back.
   - `no local copies, N remote objects` is normal, not an error: it
     means the local copy was already gone (an earlier resolve, or
     another machine) and only the remote orphan was left.
4. **Propagate the resolved state** with a single final sync:
   ```
   brain sync
   ```
5. End with the standard [additions table](#always-end-with-an-additions-table)
   listing each resolved file and a short note on what was merged.

### CSV tooling — keep it simple

The two lookup CSVs are **derived indexes**: edit `.METADATA.json`
and run reindex, don't edit the CSV by hand. For read-only queries:

- The lookup CSVs are small (projects: ~20 rows, resources: ~300 rows).
  For most questions, **just read the file** and reason over it
  directly.
- For single-key lookups, presence checks, full-text grep, use `rg`:
  ```
  rg "^<ZOTERO_KEY>," ~/brain/resources/zotero-lookup.csv
  rg 'in-progress' ~/brain/projects/projects-lookup.csv
  ```
- For multi-column filtering, sorting, or stats, use a one-shot
  Python snippet with `csv` from the stdlib. It handles quoting
  correctly and is easy to compose.

**Never write a multi-line awk/sed program to mutate a CSV** — edit
`.METADATA.json` and run reindex instead.

## Contacts

The brain's local contacts book (`<brain>/resources/contacts/`) is
owned by the sibling **[`/contacts`](../contacts/SKILL.md)** skill —
add / edit / delete / search / list people and service providers via
its deterministic CLI. When the user asks about a person or service
provider ("what's Maria's number?", "who is my accountant?", "add a
contact"), hand off to `/contacts`; don't hand-read `contacts.csv`
here. It shares this skill's additions-table and cleanup conventions.

## When something doesn't fit

If you genuinely can't tell which bucket a note belongs to, **ask the
user** before creating a new top-level folder. Do not invent new
top-level directories alongside `projects/`, `areas/`, `resources/`,
`archive/`.

## Common mistakes

| Mistake                                                    | Fix                                                         |
|------------------------------------------------------------|-------------------------------------------------------------|
| Naming a project by topic instead of outcome               | Rename to the outcome — `conference-stuff` → `apply-to-conference` |
| Creating a project folder without the `<namespace>__` prefix (`write-usage-doc` instead of `work__write-usage-doc`) | All project folders must be `<namespace>__<outcome-slug>`. See [Namespaces](#namespaces). |
| Inventing a new namespace silently (creating `projects/other__…` without asking) | Confirm any namespace outside the user's configured set (see [Namespaces](#namespaces)) with the user *before* `mkdir`. |
| Adding `namespace` to `.METADATA.json` but forgetting to update `name` / `directory` (or vice versa) | All three are tied together: `name = <namespace>__<outcome>`, `directory = projects/<name>`, `namespace = <namespace>`. Edit all three when renaming. |
| Creating a project for something with no finish line       | It's probably an area. Move to `areas/`.                    |
| Deleting a finished project to "clean up"                  | Archive it. `mv` into `archive/projects/<name>/`.           |
| Moving a resource into a project when it's referenced      | Copy the relevant excerpt; leave the resource alone.        |
| Creating `inbox/`, `notes/`, or other top-level folders    | Pick a PARA bucket, or ask the user.                        |
| `Capital_Or_Underscored_Names`                             | Lower-kebab everything: `capital-or-underscored-names`.     |
| New top-level `resources/<topic>/` when a nested home exists | Search recursively first (`fd -t d <topic> ~/brain/resources`); place under the existing parent. |
| Referencing a note as a bare path or filename (in prose)   | Use a relative markdown link so the user can jump to it.    |
| Ending a brain change without an additions table, or putting prose/links/questions after it | Every add/move/rename/archive ends with an [additions table](#always-end-with-an-additions-table) as the LAST element, paths as copyable inline-code (not links), and **home-relative** (`~/...`, or full absolute only outside home; cwd-relative paths open as URLs on iTerm2 cmd+click). |
| Skipping the neighbour search when adding a note, so real cross-links get missed | Always `fd`/`rg` for related material; add a `See also` when you find genuinely relevant notes/files/dirs. See [Cross-link new notes](#cross-link-new-notes-see-also). Padding it with tenuous links, or forcing one when nothing relevant exists, is the opposite mistake — omit it then. |
| Creating a project folder without a `.METADATA.json` (or putting `Status:`/`Due:` bullets back into `README.md`) | Status and due live in `.METADATA.json`. `README.md` keeps the H1 title + brief description only — see [Every project has a `.METADATA.json` + `README.md`](#every-project-has-a-metadatajson--readmemd). |
| Editing `projects-lookup.csv` or `zotero-lookup.csv` by hand | The CSVs are derived. Edit the relevant `.METADATA.json` and run [reindex](#reindex-the-second-brain--second-brain-reindex). |
| Archiving / renaming / deleting a project but leaving the row in `projects-lookup.csv` | Run `brain reindex --projects` so the lookup mirrors the filesystem. |
| Forgetting `extracting-ips` as a valid status | Valid statuses: `not-started`, `in-progress`, `blocked`, `extracting-ips`, `done`. |
| Marking a project `done` straight from `in-progress` / `blocked` without warning that the IP extraction phase was skipped | The whole point of `extracting-ips` is harvesting reusable work. Always offer [Extract IP](#extract-ip--extract-intermediate-packets-from-x) first — see [Mark this project as complete](#mark-this-project-as-complete--mark-project-as-done--this-project-is-done) step 2. |
| Marking a project `done` while `projects/<name>/ips/` still exists | Staged IPs were never promoted. Resolve `ips/` (promote, review, or discard) before flipping status — see [Mark this project as complete](#mark-this-project-as-complete--mark-project-as-done--this-project-is-done) step 3. |
| Archiving a project whose `.METADATA.json:status` isn't `done` | Run [Mark this project as complete](#mark-this-project-as-complete--mark-project-as-done--this-project-is-done) first so the IP-extraction safety checks fire before the folder is moved. |
| Updating `.METADATA.json` but forgetting to run reindex | After any `.METADATA.json` edit, run [reindex](#reindex-the-second-brain--second-brain-reindex) so the lookup CSV mirrors the change. |
| Using non-canonical summary/notes headings (`## AI summary`, `## Executive summary`, `## My take`) | Reindex only recognizes `## Summary` and `## Notes`. Use those exact headings; put any sub-flavoring in H3 sub-sections. |
| Reaching for `awk`/`sed` to mutate a lookup CSV | Edit `.METADATA.json` and run reindex. For read-only multi-column queries, see [CSV tooling](#csv-tooling--keep-it-simple). |
| Leaving tool byproducts (`.DS_Store`, `__pycache__/`, other tool caches) in the brain after a session | Run `brain clean` at the end of any task that touched the brain root. See [End of session: clean up tool byproducts](#end-of-session-clean-up-tool-byproducts). |
