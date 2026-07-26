# Brain sync C5 — second-brain skill integration + migration — design

- **Date:** 2026-07-25
- **Status:** Design — forks resolved with the user; ready for plan + build. The
  **final** phase of Sub-project C (C1–C4 shipped and merged).
- **Scope:** expose sync from inside a Claude session through the bundled
  `second-brain` **core** skill, and give the user's cross-machine migration a
  safe, tested foundation. Two generic skill rows (`cloud-sync`,
  `resolve-conflicts`), a structured conflict enumerator (`brain sync conflicts
  --json`), a safe copy-deleter (`brain sync resolve <original>`), a documented
  migration runbook, and a gated local round-trip integration test. **No real B2
  traffic anywhere in the build or tests.** Builds on C2 (transport, journal,
  `brain sync conflicts`), C3 (CSV merge), C4 (triggers, lock).

---

## 1. What C5 delivers

After C5, on a machine with sync configured:

- **`/second-brain cloud-sync`** runs `brain sync` from inside a Claude session and
  reports the summary, then surfaces the open-conflict count + needs-attention
  state inline and nudges the user to `/second-brain resolve-conflicts` when
  anything is open.
- **`/second-brain resolve-conflicts`** is the agent-driven resolver: it reads the
  structured `brain sync conflicts --json` list, and for each keep-both conflict
  group it reads the canonical file plus each competing copy (with host +
  timestamps), merges them into the canonical file, and calls `brain sync resolve
  <original>` to delete the now-redundant copies.
- **`brain sync conflicts --json`** emits a structured, grouped view of open
  conflict copies (each grouped under its canonical original, with parsed host,
  date, and file mtime/size), so the skill prose stays thin and the agent gets
  data, not a line-list it has to re-parse.
- **`brain sync resolve <original> [...]`** safely deletes the conflict copies that
  belong to a given canonical original (never the canonical, never a non-conflict
  file), reporting the count. Interactive picker when the original is omitted.
- The user's **migration** (§6) has a documented per-machine runbook and a gated
  local round-trip test proving an edit/add/delete round-trips A→B, the CSVs merge
  with no conflict copy, and a concurrent prose edit yields exactly one keep-both
  copy resolvable via the resolve flow.

Not in C5 (deferred, spec §19): `--check-access` marker guard, `rclone crypt`,
native-Rust `mark_done.py`, webhook endpoints, the C4 lock heartbeat, C3.3/C3.4
(the `last_touched` writer audit + `brain check` CSV-diff extension) — tracked
separately as follow-ups, not folded into C5.

## 2. Resolved design forks (settled with the user at kickoff)

| Fork | Decision |
|---|---|
| **Naming collision.** The bundled skill already maps `/second-brain sync` → `sync.py` (rebuild the lookup CSVs). | **Distinct name.** The cloud sync is a new row, **`/second-brain cloud-sync`** → `brain sync`. `/second-brain sync` (lookup rebuild) is left exactly as-is. No relearning, no churn across the skill's many `sync.py` references. |
| **How much of `resolve-conflicts` is a brain helper vs. skill prose.** | **Structured list + brain deleter.** Add `brain sync conflicts --json` (grouped/structured) and `brain sync resolve <original>` (safe, unit-tested deletion). The agent owns only the inherently-LLM part: reading the competing copies and writing the merged canonical. Enumeration and deletion are tested Rust. |
| **Does `cloud-sync` surface conflicts inline?** | **Yes, surface + nudge.** After `brain sync`, the row shows the open-conflict count + needs-attention and, when >0, tells the user to run `/second-brain resolve-conflicts`. |
| **Does `resolve-conflicts` also handle CSV soft-conflicts?** | **Prose copies only.** It resolves keep-both `(conflict …)` files. CSV soft-conflict journal notes (deleted-vs-edited / unresolvable same-field) stay journalled for a later phase, matching parent-spec §6's resolution scope. |

## 3. The structured conflict enumerator (`brain sync conflicts --json`)

Today `brain sync conflicts` prints one relative path per line (C2). C5 adds a
`--json` flag that emits a **grouped** structure keyed by the canonical original,
built on a new **pure inverse parser** of the C2 `conflict_name` builder.

### 3.1 Pure core (`src/sync/conflicts.rs`, extend)

- **`parse_conflict_name(path) -> Option<ParsedConflict>`** — the inverse of
  `conflict_name`. From a friendly copy name `stem (conflict <host> <date>).ext`
  it recovers `{ original: "stem.ext", host, date }`; the extensionless case
  (`README (conflict mac 2026-07-25)` → `README`) mirrors the forward builder.
  Returns `None` for a name that isn't a friendly conflict copy. This is the
  crown-jewel pure function of C5, tested as a strict round-trip against
  `conflict_name` (build → parse → equal for a matrix of stems/exts/hosts/dates).
- **`group_conflicts(files: &[ConflictFile]) -> Vec<ConflictGroup>`** — fold the
  flat `list_conflicts` output into groups by recovered `original`, each carrying
  its copies (path + parsed host/date). Pure; deterministic order (by original,
  then copy path) so JSON output is stable.
- **`copies_for_original(original, files) -> Vec<PathBuf>`** — the pure predicate
  behind `resolve`: which of the open conflict copies belong to this canonical
  original. Never returns the original itself.

`ConflictGroup` / `ParsedConflict` are small `#[derive(Serialize)]` structs so the
CLI layer only has to attach the impure bits (mtime, size, `original_exists`) and
serialize.

### 3.2 CLI + output (`src/cli.rs`, `src/sync/command.rs`, `src/main.rs`)

- `SyncAction::Conflicts` gains `{ #[arg(long)] json: bool }`.
- `print_conflicts(root, json)`:
  - `json=false` → today's themed line-list (unchanged).
  - `json=true` → a JSON array of groups. Each group:
    ```jsonc
    {
      "original": "resources/ai/idea.md",
      "original_exists": true,
      "copies": [
        { "path": "resources/ai/idea (conflict mac 2026-07-25).md",
          "host": "mac", "date": "2026-07-25",
          "modified": "2026-07-25T10:04:11Z", "bytes": 1841 }
      ]
    }
    ```
  - JSON goes to **stdout** (it's machine output the agent consumes), which is the
    one sanctioned non-`brain config` stdout use: a structured data command,
    exactly like the config table. Empty list → `[]`. Errors reading a file's
    metadata degrade gracefully (`modified`/`bytes` become `null`), never abort the
    listing.
- Paths are **relative to the brain root** (as `list_conflicts` already returns),
  so the JSON is host-agnostic and the skill resolves them under `<brain>`.

## 4. The safe deleter (`brain sync resolve <original>`)

- **New `SyncAction::Resolve { originals: Vec<String> }`.** For each original,
  compute `copies_for_original` against the live conflict set and delete exactly
  those copies. Prints a themed one-line summary (`resolved <original>: removed N
  copies`). Never touches the canonical file and never a file that doesn't parse as
  one of *its* conflict copies.
- **Guard:** if the named canonical original does **not** exist on disk, refuse
  with a themed warning (`the canonical file doesn't exist — merge into it before
  resolving`) and delete nothing. The resolve step is meant to run *after* the
  agent has written the merged canonical; deleting the copies while the canonical
  is missing would destroy the only remaining content.
- **Decided: `resolve` is a pure local delete, no sync of its own.** It removes the
  copies and returns; it never runs `brain sync`. The `resolve-conflicts` skill row
  runs **one** `brain sync` at the very end (after every group is resolved) to
  propagate the whole result at once. This keeps `resolve` single-purpose, avoids N
  redundant syncs for N conflicts, and never blocks the delete on the network.
- **Human-friendly fallback (house rule):** bare `brain sync resolve` with no
  originals drops into an interactive picker over the open conflict groups (themed
  prompt), so a human isn't forced to type a path. The agent path always passes the
  original(s) explicitly.
- Pure `copies_for_original` is unit-tested; the FS delete + picker are the thin
  shell.

## 5. The two skill rows (bundled `second-brain` **core**, 100% generic)

Both land in `skills/second-brain/SKILL.md`. They must pass
`bundled_skills_carry_no_personal_data` (no bucket names, hosts, emails, org names,
private paths). All commands are the generic `brain sync …` surface; `<brain>` is
resolved via `brain config get root` as the rest of the skill already does.

### 5.1 `### Cloud-sync the brain / /second-brain cloud-sync`

- **Trigger:** "cloud-sync", "push my brain to the cloud", "pull the latest brain",
  "sync across machines". Explicitly distinguished in-prose from the existing
  lookup-rebuild `/second-brain sync` (a callout notes the two are different: one
  rebuilds derived CSVs, this one syncs files across machines via `brain sync`).
- **Steps:** run `brain sync` (optionally `--push` / `--pull`), echo its summary;
  then run `brain sync status` and surface the **open-conflict count** and
  needs-attention state. If open conflicts > 0, tell the user to run
  `/second-brain resolve-conflicts`. End with the skill's standard additions table.
- Notes that sync is opt-in: if it prints "sync is not configured", point at
  `brain sync setup` and stop.

### 5.2 `### Resolve sync conflicts / /second-brain resolve-conflicts`

- **Trigger:** "resolve conflicts", "fix the sync conflicts", "merge the conflict
  copies".
- **Steps:**
  1. `brain sync conflicts --json` → the grouped list. If empty, say so and stop.
  2. For each group: read the canonical `original` (if it exists) and each competing
     copy under `<brain>`, using the parsed `host` + `date` + `modified` as the
     recency signal. Merge the divergent content into the canonical file
     (LLM judgment: union of edits, newest wins on a true clash, preserve both
     where they're additive), following the second-brain content conventions.
  3. `brain sync resolve <original>` to delete that group's copies once the merge is
     written. Repeat per group.
  4. Run `brain sync` once at the end so the resolved state (canonical kept, copies
     removed) propagates.
  5. End with the additions table listing each resolved file + what was merged.
- Explicitly **scoped to prose keep-both copies**; a note says CSV divergences are
  handled automatically by the merge and any residual soft-conflicts show only in
  the journal (out of scope here).

## 6. Migration (parent-spec §14) — safe, no live B2

C5 delivers the migration as a **documented runbook + a gated local test**, never a
live B2 run in this environment (the real bucket is 143 GB).

- **Verify C1 auto-migration** (pointer → env `root`): confirm the existing C1
  behavior with a test that seeds a throwaway `HOME`/`XDG_CONFIG_HOME`, writes a
  legacy `~/.config/brain-root` pointer, and asserts first-run resolution migrates
  it into `~/.config/brain/env.json`'s `root` key. If C1 already covers this, cite
  the test; otherwise add it. `markdown_to_pdf_path` env move likewise.
- **Runbook (`docs/…` / migration section):** the per-machine steps the user runs
  by hand — create the B2 bucket once; `brain sync setup` on each machine; enable
  triggers; the A→B verification checklist from parent-spec §14.4. Documentation,
  not automation.
- **Gated local round-trip test (`tests/sync_local.rs`, extend):** using rclone's
  **local backend** (a throwaway `HOME`/root, `#[ignore]`-gated like C2/C3/C4's
  local tests), simulate two machines through a shared "remote" directory and
  assert:
  1. an **edit / add / delete** on side A appears on side B after a sync;
  2. `tasks.csv` / `habits.csv` diverging edits **merge with no `(conflict …)`
     copy** (Lane B);
  3. a **concurrent prose edit** yields **exactly one** keep-both copy, and the
     `resolve` flow (`conflicts --json` → delete via `copies_for_original`) leaves
     only the canonical.
  Never points at B2; all real-path checks sandboxed under the throwaway HOME/XDG.

## 7. Module + surface touch-list

| File | Change | Pure? |
|---|---|---|
| `src/sync/conflicts.rs` | `parse_conflict_name` (inverse), `group_conflicts`, `copies_for_original`, `ParsedConflict`/`ConflictGroup` (Serialize) | pure |
| `src/cli.rs` | `Conflicts { json: bool }`; new `Resolve { originals: Vec<String> }` | — |
| `src/sync/command.rs` | `print_conflicts(root, json)` (JSON branch); `resolve(root, &originals)` + interactive picker | thin over pure |
| `src/main.rs` | dispatch `--json` + `Resolve` | thin |
| `skills/second-brain/SKILL.md` | the two generic rows (§5) | — |
| `tests/sync_local.rs` | round-trip + resolve assertions (gated) | test |
| `docs/*`, `AGENTS.md` | §8 | — |

No new crate (JSON via the existing `serde_json`). No new keybinding, palette, or
menu row (skills are Claude-session commands, not TUI actions), so the
keybinding/palette docs are untouched.

## 8. Docs to update (same change)

- `docs/features.md` — `/second-brain cloud-sync` + `resolve-conflicts`; `brain
  sync conflicts --json`; `brain sync resolve`; the inline conflict nudge.
- `docs/integrations.md` — the structured conflict enumerator + resolve deleter as
  the skill's brain-side contract; the migration runbook.
- `docs/data-model.md` — the `conflicts --json` group schema (`ParsedConflict` /
  `ConflictGroup`).
- `docs/architecture.md` — the enumerator/resolver surface in `src/sync/`.
- `docs/decisions.md` — distinct `cloud-sync` name (why not clobber `sync`);
  structured-list-plus-brain-deleter over pure prose; resolve's canonical-exists
  guard; prose-only resolution scope for C5.
- `docs/config.md` — only if the runbook lives here; otherwise leave.
- The docs-contract table in `AGENTS.md`/`CLAUDE.md` — a C5 row: "the second-brain
  sync skill rows (`cloud-sync`, `resolve-conflicts`), `brain sync conflicts
  --json`, and `brain sync resolve`" → `docs/features.md` + `docs/integrations.md`
  + `docs/data-model.md` (`parse_conflict_name`/`group_conflicts`/
  `copies_for_original` in `src/sync/conflicts.rs`; the bundled rows in
  `skills/second-brain/SKILL.md`).

## 9. Testing (pure-first, per house rules)

- **`parse_conflict_name`** (crown jewel): round-trips `conflict_name` for a matrix
  of stems / extensions / extensionless / multi-word stems / hosts / dates; rejects
  non-conflict names, the marker shape, and a stem that merely contains
  "(conflict ...)" text but doesn't match the exact grammar.
- **`group_conflicts`**: multiple copies of one original group together; distinct
  originals stay separate; deterministic ordering.
- **`copies_for_original`**: returns only that original's copies; never the
  canonical; empty when none match.
- **`resolve` guard**: refuses when the canonical is missing (deletes nothing).
- **`conflicts --json`** shape: `[]` when clean; a populated group serializes with
  the documented keys; metadata-read failure degrades to `null` fields.
- **Guard test stays green**: the two new skill rows carry no personal token.
- **Gated `tests/sync_local.rs`**: §6's round-trip + resolve, rclone local backend,
  `#[ignore]`, throwaway HOME/XDG — never B2.
- Skill prose, the interactive picker, and the FS delete shell are **not**
  unit-tested directly (per strategy); their decision logic is the pure functions
  above.

## 10. Phase decomposition (for the C5 plan — each a RED→GREEN slice)

- **C5.1 — Pure inverse + grouping.** `parse_conflict_name`, `group_conflicts`,
  `copies_for_original`, `ParsedConflict`/`ConflictGroup`. Fully unit-tested; no CLI
  yet.
- **C5.2 — `conflicts --json`.** The `--json` flag + grouped stdout output built on
  C5.1 (mtime/size/`original_exists` attached at the shell).
- **C5.3 — `brain sync resolve`.** `Resolve` subcommand, canonical-exists guard,
  pure `copies_for_original` deletion, interactive-picker fallback.
- **C5.4 — Skill rows.** The two generic rows in `skills/second-brain/SKILL.md`;
  guard-test green; `brain skills sync` re-render sanity.
- **C5.5 — Migration.** Verify (or add) the C1 auto-migration test; the runbook doc; the
  gated local round-trip + resolve test.
- **C5.6 — Docs + housekeeping.** §8 docs; the AGENTS.md contract row; delete the
  stray `.difit/*` review files from the merged `feat-cli-polish`,
  `feat-personalize`, `feat-sync-progress`, `refactor_modularize` branches.

## 11. Acceptance criteria

1. `/second-brain cloud-sync` runs `brain sync`, reports the summary, and surfaces
   the open-conflict count + needs-attention inline, nudging to `resolve-conflicts`
   when >0. It is a **new** row; the existing lookup-rebuild `/second-brain sync` is
   unchanged.
2. `/second-brain resolve-conflicts` walks `brain sync conflicts --json`, merges
   each group into its canonical file, and removes the copies via `brain sync
   resolve`, leaving exactly the canonical.
3. `brain sync conflicts --json` emits the documented grouped schema (relative
   paths, parsed host/date, mtime/size); `brain sync resolve <original>` deletes
   only that original's copies and refuses when the canonical is missing.
4. The C1 root/`markdown_to_pdf_path` migration is proven by test; the migration
   runbook is documented; the gated local round-trip proves edit/add/delete A→B,
   CSV merge with no conflict copy, and exactly-one-keep-both resolvable via the
   resolve flow. **No real B2 traffic in any test.**
5. The repo stays generic (guard test green); no bucket/host/key/personal path in
   the bundle or repo.
6. `cargo test --release` green; `cargo clippy --release --all-targets` clean; docs
   updated in the same change.

## 12. Open questions (resolve in the plan / at build)

- The exact friendly-name grammar `parse_conflict_name` must accept: today
  `conflict_name` uses a single space-delimited ` (conflict <host> <date>)` before
  the extension. Confirm host tokens never contain `)` or ` (conflict ` (hostname
  is unqualified + trimmed in `command::hostname`, so safe) and pin the regex/parse
  to that grammar at C5.1.
- Whether the runbook lives in `docs/features.md` (sync section) or a dedicated
  `docs/migration.md`. Lean features.md to avoid a near-empty file; confirm at C5.5.
