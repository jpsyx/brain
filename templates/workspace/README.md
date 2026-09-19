# brain workspace

A personal knowledge base, organized using Tiago Forte's PARA method.

Everything in this directory is one of four kinds of thing, plus an
in-basket that feeds them:

- **`projects/`** — short-term efforts with a defined outcome and (usually) a
  deadline. Name each project for the outcome it produces
  (`launch-team-handbook`, not `handbook-stuff`).
- **`areas/`** — ongoing responsibilities to maintain over time, with no finish
  line (`health`, `finances`, `team-leadership`).
- **`resources/`** — topics or reference material that may be useful one day,
  not tied to a current project or area (`python-tips`, `system-design`).
- **`archive/`** — anything from the three above that is no longer active. Move
  things here instead of deleting them; archives are searchable history.

Plus two non-PARA directories:

- **`capture/`** — **your** in-basket. Dump anything here: a half-written
  thought, a photo, a screenshot, a PDF, a folder of the above, named however
  you like. Nothing else in this workspace is yours to keep messy; this one is.
  Later, ask the agent to "process my capture" and it files each item into the
  right PARA folder or turns it into a task.
- **`tasks/`** — the task system: tasks, habits, and the schema documenting
  them. Managed through the `todo` and `triage` skills, or the `brain` CLI.

## How things flow

```
capture/ ──► projects/ | areas/ | resources/   (when it gets processed)
         └─► tasks/                            (when it turns out to be a to-do)

resources/ ─┐
            ├─► projects/ ──► archive/   (when the project finishes)
areas/ ─────┘                 archive/   (when the area no longer applies)
            └─► areas/ ─────► archive/
```

A note may start life in `capture/`, get filed into `resources/`, get pulled
into a `projects/` folder when it becomes actively useful, and end up in
`archive/` once that project completes.

## Conventions

- All directory and file names are **lower-case and kebab-case**, so they are
  easy to type in a terminal. `capture/` is exempt: it is yours, and the
  conventions get applied when something moves out of it.
- Everything is **plain text** where possible (markdown, csv, json), so it works
  with ordinary command-line tools.
- Prefer **archiving over deleting**. Disk is cheap; lost context is not.
- A note about a specific file (a PDF, an image, a dataset) lives in the **same
  subdirectory as that file**, so it is always clear which note describes what.

## Working here with an agent

[AGENTS.md](AGENTS.md) holds the instructions agents follow in this workspace.
The `second-brain` skill is the playbook for deciding where new material belongs;
invoke it before adding or reorganizing anything.
