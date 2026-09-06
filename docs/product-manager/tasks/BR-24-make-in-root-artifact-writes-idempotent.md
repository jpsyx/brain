---
id: BR-24
title: Make in-root artifact writes idempotent
status: todo
priority: high
assignee: jpsyx
labels: [bug, enhancement, sync, performance]
estimate: 5
project: PROJ-2
milestone: MS-7
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-24: Make in-root artifact writes idempotent

## Description

Three Brain writers rewrite files inside the synced root with identical content
on every invocation, bumping mtimes for no content reason. Each one guarantees a
`File changed: time (newer)` delta on the next bisync, re-uploads the bytes,
retriggers the filesystem watcher, and arms BR-23's race window.

**The skills pipeline is the worst of the three.** `write_built_to` in
`src/skills/install.rs` calls `fs::remove_dir_all` on
`<root>/.agents/skills/<name>` and then rewrites every rendered file. This runs
on every TUI launch, every config mutation, and every version change. Measured:
33 files, roughly 1 to 2 MB, per launch. Note that a per-file byte-equality
check alone is a **no-op** here, because the directory is deleted first; the
`remove_dir_all` has to go, replaced by a reconciling write that compares each
file and prunes only what the current render no longer produces. That also
closes a second hazard: the deletion window lets a concurrently running rclone
listing observe 33 files vanish and propagate the deletes.

**`RCLONE_TEST` is re-stamped on every single sync.**
`check_access::ensure_local_marker` does an unconditional `fs::write` of fixed
content and is called from `sync_once` for every non-Push direction. Fixed
content, rewritten every run, therefore a path1 delta every run. It must **not**
be excluded from sync instead: `--check-access --check-filename RCLONE_TEST`
verifies the marker in both listings, so excluding it makes every sync abort and
then auto-repair in a loop (measured).

**The reindex lookups are unconditional rewrites of real user content.**
`src/reindex/walk.rs` writes `projects/projects-lookup.csv` and
`resources/zotero-lookup.csv` with `wrote: true` hardcoded, so an identical
rebuild still produces a push. One of these appeared as
`File changed: size (smaller), time (newer)` in the investigation's logs.

Brain already has the right pattern seven times over: `needs_rewrite` in
`src/command/server/receiver/hooks/artifact.rs`, plus read-and-compare in
`hooks/json.rs`, `csv_sync/operation.rs`, `csv_sync/metadata.rs`,
`counters.rs`, and two `triage_habits` modules. Add one shared idempotent-write
helper rather than an eighth copy.

## Acceptance criteria

- [ ] `write_built_to` no longer calls `remove_dir_all`. It compares each rendered file against what is on disk, writes only real differences, and prunes exactly the files the current render no longer produces.
- [ ] A launch that changes no skill content modifies no file mtime under `<root>/.agents/skills/`, asserted by a test that renders twice and compares mtimes.
- [ ] `ensure_local_marker` writes only when the marker's content differs, and `RCLONE_TEST` is **not** added to `EXCLUDES`. A guard test asserts no `EXCLUDES` pattern matches `RCLONE_TEST`, so a future change cannot reintroduce the abort loop.
- [ ] The two reindex lookup CSVs are written only on a real content change, and `Report.wrote` reflects what actually happened rather than a hardcoded `true`.
- [ ] One shared idempotent-write helper backs all of these, and the existing seven read-and-compare sites are either migrated to it or explicitly left alone with a reason.
- [ ] Hand-authored skills are unaffected. Five skills in the author's workspace exist only under `.agents/skills/` with no `.brain-rendered` marker, and `email-triage/config/` holds live agent-authored state; a test proves an unmarked directory survives a render of the marked ones.
- [ ] `docs/architecture.md`'s skills-pipeline section describes the reconciling write, replacing the nuke-and-rewrite description, and `docs/decisions.md` records why identical renders must not touch mtimes.

## Notes

This is an accuracy fix first and a speed fix second. It removes roughly 33
uploads per launch and their listing deltas, and it shrinks BR-23's race window,
but it does **not** close that window: BR-23 is still required.

Do not "fix" this by excluding the rendered tree from sync. That was proposed,
reviewed, and rejected for causing data loss; the investigation records why.

### Pointers (as of 2026-09-05)

- `src/skills/install.rs` — `write_built_to` (the `remove_dir_all` plus rewrite) and `write_fresh_built_to`. Note the latter targets the UUID cache via `capability_skills_dir`, not the synced root, so making it idempotent has no sync effect; scope the work to the root-targeting path.
- `src/skills/prune.rs` — `RENDERED_MARKER` (`.brain-rendered`) and the pruning pass that decides a directory is Brain's to delete by the marker's presence. The reconciling write has to keep that contract intact. Note the marker itself is already a sync exclude via the unanchored `.brain-*` pattern.
- `src/skills/layout.rs` — `Layout::real` resolves `built_dir == agents_dir == <root>/.agents/skills`, which is why the rendered tree and hand-authored skills share one directory. That mixing is the reason exclusion is unsafe.
- `src/command/server/receiver/hooks/artifact.rs` — `needs_rewrite` and `write_static_file`; the pattern to extract into the shared helper, including its atomic temp-file-then-rename and mode handling.
- `src/sync/check_access.rs` — `ensure_local_marker`, `CHECK_FILENAME`, `MARKER_CONTENT`, and `ensure_markers_with`. The guard goes in the local-marker write.
- `src/sync/args.rs` — the `EXCLUDES` array and its long doc comment explaining why each entry exists. The guard test lives near its existing tests; read the comment before touching the array.
- `src/reindex/walk.rs` — the two `fs::write` calls and the `Report { wrote: true }` construction.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — the "Candidate fixes, adjudicated" row for this task, and the rejected-exclusion row directly below it.

### Log

- 2026-09-05 created from the investigation.
