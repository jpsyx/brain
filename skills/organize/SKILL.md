---
name: organize
description: File and retrieve Brain notes or resources, choose PARA placement, pair media with indexed notes, repair links, or rebuild local lookup indexes. Cloud sync belongs to brain-sync.
---

# organize

Own PARA placement, resource/media ingestion, retrieval, cross-links, and local
reindex decisions. Project lifecycle belongs to [`/projects`](../projects/SKILL.md);
inbox processing belongs to [`/process-capture`](../process-capture/SKILL.md).

## Placement

Organize by actionability: an active finite outcome belongs in `projects/`, an
ongoing responsibility in `areas/`, and reusable reference material in
`resources/`. Resources are grouped by topic, including nested topics. Inactive
material preserves its original bucket path under `archive/`. An action goes
to [`/todo`](../todo/SKILL.md). `capture/` is never a processed destination.

Search recursively for existing homes and relevant neighbors before creating
folders. Honor an explicit destination; otherwise choose and state a routine
placement. An explicit filing, processing, moving, or renaming request
already authorizes those routine decisions. Ask only for unresolved
consequential ambiguity, destructive disposition, or a new configured namespace.
Do not invent a new top-level bucket. For a finite outcome needing a new
project, use the project creation workflow.

Use descriptive lower-kebab filenames for standalone notes, domain names for
areas, and topic names for resources. Keep recognized canonical filenames
such as `.METADATA.json`, `README.md`, and indexed `notes.md`. When using a
resource in active work, copy the relevant excerpt into the project and keep
the original resource in its reference home.

## Add a resource

For `/organize add-resource` or "save this":

1. Read the artifact and identify its topic; use the user's topic if supplied.
2. Search existing resource directories recursively, including synonyms.
   Prefer the natural existing parent over a duplicate top-level topic.
3. Place the artifact. Retain original PDFs/images/downloaded HTML. For URLs,
   save a useful snapshot or a note carrying the URL and takeaways. A new
   standalone prose note normally gets a descriptive file; append only for
   the same narrow subject/source.
4. When media has commentary, use
   [paired media and notes](references/resources.md#paired-media-and-notes).
   Substantive prose gets a summary using
   [`/article-summarizer`](../article-summarizer/SKILL.md), with the canonical
   saved-summary structure. Skip distillation for reference docs, cheat sheets,
   datasets, code, diagrams, or short original notes whose whole content is
   the value.
5. Search for meaningful neighbors, cross-link useful notes/files/directories,
   and repair affected links after moves. Omit `See also` if no real match exists.
6. Reindex only affected indexed metadata/media notes as described below.
   Report the saved material, provenance, destination, and any processing failure.

<!-- brain:ext organize:reference-manager -->

## Retrieval

For `/organize find`, "what have I read about X?", or "find that note":
search all applicable buckets: `projects/`, `areas/`, `resources/`, `archive/`,
and `capture/`. Start with `resources/zotero-lookup.csv` when available for
fast title/tag matches, then search full text and filenames across the relevant
buckets (`rg` and `fd` are useful). Do not limit an answer to indexed resources.
Read candidates before answering. Synthesize the findings, identify their
sources and relevance, and flag capture hits as unfiled without processing them.
If nothing supports an answer, say so. Do not invent material to fill a result.

## Reindex

`/organize reindex` handles explicit "reindex", "rebuild the lookups", or
"refresh derived CSVs" requests. Bare "sync" means
[`/brain-sync`](../brain-sync/SKILL.md#cloud-sync), a different operation.

```sh
brain reindex
brain reindex --projects
brain reindex --resources
brain reindex --tasks
```

The native command is the only bulk index rebuild. Project commands already
rebuild their lookup. For resource metadata, attachment, or indexed `notes.md`
changes, run `brain reindex --resources`; a standalone prose note without
indexed metadata needs no rebuild. Native task/habit mutations apply their
own rules and agenda synchronization. Explicit `--tasks` maintenance runs
native task rules, managed-triage reconciliation, and completed-habit cleanup;
see the task owner's [sync rules](../todo/references/sync-rules.md).

Read [resource indexing](references/resources.md#index-fields) for summary,
note, and annotation fields. Never hand-edit derived lookup CSVs. Read small
indexes directly or use `rg` for simple lookups and Python's standard `csv`
module for quoted multi-column filtering. Do not mutate CSVs with awk/sed.
