# Resource notes and indexing

## Paired media and notes

An artifact with commentary gets one subject directory:

```text
resources/<topic>/<subject>/
  <subject>.pdf
  notes.md
```

The same shape works for HTML, images, datasets, and reference-manager imports.
Keep originals with their commentary. Use a descriptive filename for an
unpaired standalone note; do not rename every arbitrary note to `notes.md`.

In indexed `notes.md`, use exactly `## Summary` for source distillation,
`## Notes` for other notes, and `## Annotations` for extracted annotations.
Use H3 subheadings for variations, not alternate H2 names such as "AI summary".
The [article summarizer](../../article-summarizer/SKILL.md#output-structure)
owns the saved summary's internal structure and source-fidelity rules.
Record source URL or other provenance so excerpts remain traceable.

## Index fields

`brain reindex --resources` rebuilds `resources/zotero-lookup.csv` from resource
`.METADATA.json` records and colocated `notes.md`:

| Derived field | Source |
| --- | --- |
| `directory` | Current filesystem location |
| `has_pdf`, `has_html` | Metadata `attachments` |
| `has_summary` | Nonempty canonical `## Summary` |
| `has_other_notes` | Nonempty `## Notes`, excluding empty/placeholder sentinels |
| `annotation_count` | Distinct blockquote blocks and `*(ink annotation)*` entries under `## Annotations` |

Reindex after changing indexed notes, resource metadata, attachments, or a
resource directory name. Plain unindexed prose does not need reindexing.
Use [organize](../SKILL.md) for placement and retrieval; resource indexes never
replace a search of unindexed notes, active projects, areas, archive, or capture.
