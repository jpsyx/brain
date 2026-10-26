# Extract IPs

An Intermediate Packet (IP) is a reusable building block: distilled notes,
an outline or template, a draft, a diagram, or a research/decision artifact.
Keep project-specific work in the project. Promote independent reusable
material into an area or resource where future work can find it.

1. Identify one source project. Ask if it is unspecified; do not harvest
   several projects in one pass.
2. Run `brain project set <namespace>__<outcome> --status extracting-ips`.
   The native command updates metadata and the lookup together.
3. Copy candidates into `projects/<slug>/ips/`, preserving their filenames
   and the originals in the project. Record each proposed destination beside
   the candidate or in `ips/README.md`. Include provenance back to the source.
4. Give the user the staged inventory, its location, and proposed destinations.
   They can edit, rename, or remove candidates before promotion. Stop for
   review; staging is not permission to promote.
5. On approval, move only the approved staged copies into their destinations.
   Use [`/organize`](../../organize/SKILL.md) for placement, media pairing,
   neighbor search, and link repair. Approval to file/promote authorizes
   routine destination creation; ask only for unresolved consequential
   choices or a new configured namespace, not each routine directory.
6. Account for every candidate as promoted, explicitly rejected, or still
   awaiting review. Remove the staging directory when empty; leave it if the
   user wants to continue review. Destructive disposal requires explicit consent.
7. Once all staged IPs are resolved, use [Complete](../SKILL.md#complete)
   and `brain project set <slug> --status done`. Do not hand-edit metadata.
   The project can then be archived through [Archive](../SKILL.md#archive).
8. Report promoted destinations and anything still staged. Originals remain
   in project history, so archiving never loses the working context.
