# Project metadata

Use native `brain project new`, `set`, `show`, and `archive` commands.
The canonical `.METADATA.json` contains:

| Field | Meaning |
| --- | --- |
| `name` | Full `<namespace>__<outcome-slug>`, matching the folder |
| `namespace` | Configured namespace prefix, allowing lowercase letters, digits, and hyphens |
| `title` | Human-readable outcome; initially the README H1 |
| `status` | `not-started`, `in-progress`, `blocked`, `extracting-ips`, or `done` |
| `priority` | Required `p0`, `p1`, `p2`, `p3`, or `p4` |
| `due` | Absolute `YYYY-MM-DD`, or `none` |
| `directory` | Root-relative current path, including the namespace prefix |
| `tasks` | Linked short task/habit IDs, maintained through native commands |

`not-started` means no work has begun; `in-progress` means active work;
`blocked` means waiting on something (explain the blocker in working notes).
`extracting-ips` is the harvest phase before archiving. `done` means the
outcome is achieved or abandoned and reusable work has been resolved through
[the completion gates](../SKILL.md#complete).

Use `p0` for a near hard deadline or critical blocker, `p1` for a committed
current focus, `p2` for active work without immediate pressure, `p3` for work
that can wait for capacity, and `p4` for parked work. Resolve the user's
priority decision rather than silently assigning one.

A new README contains an H1 and one to three sentences describing the outcome.
Longer working material belongs in `notes.md` or other descriptive files.
The native commands maintain identity/path fields and preserve unknown metadata.
See [task-project linkage](task-project-link.md) for the bidirectional invariant.
