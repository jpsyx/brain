# Agenda artifacts

## Resolve destinations

In the selected workspace/actor environment, read:

```sh
brain env get agenda_markdown_dir
brain config get agenda_dir
```

Expand a leading tilde using the current home directory, matching native path
resolution. Write `<agenda_markdown_dir>/<YYYY-MM-DD>.md`; the printable is
`<agenda_dir>/agenda-<YYYY-MM-DD>.pdf`. Use these resolved destinations for
existence checks, updates, rendering, and links. Never substitute a fixed directory
or a shell alias. Create the Markdown directory if needed. Both settings belong to
the selected workspace; the PDF renderer is configured through
`brain env get markdown_to_pdf_path` and invoked by the native command below.

## Document structure

Write a presentation-ready plan in this order:

1. `# YYYY-MM-DD - <weekday> agenda`
2. `**Load:**` one short descriptor/sentence, without task IDs or a changelog.
3. `**Bottom line:**` one statement, at most two; pair every ID with its name.
4. `## ❗ MITs - if only these get done, today is a win`
5. `## Suggested order`
6. `## Cut order if the day slips`
7. `## 🔁 Today's habits` (omit when empty)
8. `## ✅ Completed today` (omit when empty)

Separate paragraphs, lists, and sections with blank lines. Keep the title/load,
MITs, Suggested order, and Cut order on page 1 where feasible; reference tables
follow. Add no extra narrative sections or mutation audit log. Preserve an existing
appendix or user/extension-owned narrative when updating; these are not permission
to invent extra core sections.

MITs use flat `- [ ] ❗ **T###** name (duration)` lines in execution order.
Suggested order uses `1. [ ] ~<time> | **T###** name (duration)` with continuous
numbering over actionable items. Use approximate times at no finer than half-hour
granularity for tasks; exact fixed events retain their actual times. Scheduled
non-task anchors and calendar events are plain `<time> | <name>` lines, unnumbered
and unchecked, in chronological position. Cut order uses plain numbered lines,
no checkboxes, at most five items. MITs appear both in their callout and sequence.
Use ❗ for MIT, 🔥 for a critical path, ⚠️ for hard deadlines, ✅ for completion.

Habits and Completed today are two-column Markdown tables with blank header cells.
Each cell pairs ID and name. Pending habits use `◻`, completed rows `✅`. Habits
include pending rows with empty/elapsed due dates plus habits completed on the
target date; honor any start boundary. Pending first, then done-on-date, each sorted
by `ideal_time` (empty last), then duration, then name, filled left-to-right.
Completed today includes every task and habit with `status=done` and
`completed_date=target date`. Never include drops, defers, conversions, or adds.

For a requested plan update, reread both CSVs and derive these tables again. Remove
completed IDs from Suggested order, renumber, recompute durations/load, and remove
stale labels or contradictory estimates. Keep settled planning choices. A native
mutation is a different path and must not trigger this freehand rewrite.

## Native mutation synchronization

Rust owns synchronization after task/habit mutations, including managed-triage
completion and cadence-aware skip/defer. No agenda file means no new agenda. It
surgically updates action lines, advances the next unfinished chunk when applicable,
rebuilds CSV-derived tables, and preserves unrelated narrative/appendix bytes.
It refreshes the PDF only if it already exists. Failures are downstream warnings,
not evidence that the canonical mutation failed. Do not retry a committed mutation
or rewrite its agenda yourself. An explicit `brain tasks sync-agenda` can refresh
derived sections when requested; do not reconstruct this implementation in a skill.

## Render and verify

After an intentional build or plan revision, generate a printable:

```sh
brain tasks agenda-pdf --date <YYYY-MM-DD>
```

The native command resolves both destinations and the configured renderer, strips
HTML comments from a fresh render copy, removes the prior PDF to avoid versioned
collisions, passes `--agenda` for compact borderless tables, and disposes of its
render copy. Preserve source comments such as appendix idempotency markers. Do not
invoke a shell alias or duplicate these steps. The mutation path's existing-PDF
rule above is the exception to deliberate build/revision rendering.

Verify page count and layout using the actual resolved PDF path. Target two
US-Letter pages at standard margins/font. Completed today may spill onto page 3.
If the body exceeds two pages, trim in this order: habit cell names, Suggested
order names, MIT names, then the entire Cut order section. Retain IDs, times, and
durations. If still over, accept it. Never use `--font-shrink`, remove load-bearing
MIT/Suggested/habit sections, or sacrifice legibility. Re-run the native PDF command
after each trim. Do not open the PDF unless asked; on macOS use `/usr/bin/open`
with the resolved path. Finish with the [post-build hook](../SKILL.md#after-build).
