# Brain Sync C5 — second-brain skill integration + migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Follow the repo's iron-law TDD: write the failing test, run it, watch it fail, then make it pass.

**Goal:** Expose sync from inside a Claude session and give the user's cross-machine migration a tested foundation. Two generic bundled-skill rows (`/second-brain cloud-sync`, `/second-brain resolve-conflicts`), a structured conflict enumerator (`brain sync conflicts --json`) built on a **pure inverse** of `conflict_name`, a safe copy-deleter (`brain sync resolve <original>`), the migration runbook, and a gated local round-trip + resolve test. **No live B2 anywhere.**

**Architecture:** Everything data-shaped is a pure function in `src/sync/conflicts.rs` and exhaustively tested: `parse_conflict_name` (the crown jewel, strict round-trip of the C2 `conflict_name` builder), `group_conflicts`, `copies_for_original`. The CLI/command layer (`command.rs`, `main.rs`, `cli.rs`) is a thin shell that attaches the impure bits (file mtime/size, `original_exists`, the interactive picker, the FS delete) and serializes. `resolve` is a pure local delete with **no** sync of its own; the skill row runs one `brain sync` at the end. The two skill rows are generic prose in the bundled core skill, guarded by `bundled_skills_carry_no_personal_data`.

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json` (already deps — no new crate), `clap`, `walkdir` (already used by `conflicts.rs`). `cargo test --release` + `cargo clippy --release --all-targets`. The gated integration test uses rclone's **local backend** (like `tests/sync_local.rs`), `#[ignore]` / PATH-gated, throwaway `HOME`/`XDG_CONFIG_HOME` — never B2.

---

## Scope

Phase **C5** of the [brain-sync spec](../specs/2026-07-24-brain-sync-design.md), detailed in the [C5 spec](../specs/2026-07-25-brain-sync-c5-skill-integration.md). Builds on **C2** (transport, journal, `conflict_name`, `list_conflicts`, `print_conflicts`), **C3** (CSV merge), and **C4** (triggers, lock), all merged to `main`.

**In scope:** the pure inverse parser + grouping + `copies_for_original`; `brain sync conflicts --json`; `brain sync resolve <original>` (+ interactive fallback); the two generic skill rows; the C1-migration verification test; the migration runbook; the gated local round-trip + resolve test; the docs + AGENTS.md contract row.

**Out of scope (deferred):** CSV soft-conflict resolution in the skill (prose copies only for C5); C3.3 (`last_touched` writer audit); C3.4 (`brain check` CSV diff); the C4 lock heartbeat; spec §19 backlog (`--check-access`, `rclone crypt`, native `mark_done.py`, webhooks). The stray `.difit/*` cleanup listed in spec §10 C5.6 is **already done** (removed before the build).

## File Structure

| File | Responsibility | Pure? |
| --- | --- | --- |
| `src/sync/conflicts.rs` (edit) | `parse_conflict_name` (inverse of `conflict_name`), `group_conflicts`, `copies_for_original`, `ParsedConflict`/`ConflictGroup` structs (`Serialize`) | pure |
| `src/cli.rs` (edit) | `Conflicts { json: bool }`; new `Resolve { originals: Vec<String> }` | — |
| `src/sync/command.rs` (edit) | `print_conflicts(root, json)` JSON branch; `resolve(root, &originals)` + interactive picker | thin over pure |
| `src/main.rs` (edit) | dispatch `--json` + `Resolve` | thin |
| `skills/second-brain/SKILL.md` (edit) | the two generic rows (`cloud-sync`, `resolve-conflicts`) | — |
| `tests/sync_local.rs` (edit) | gated round-trip (edit/add/delete A→B, CSV merge no-conflict) + resolve assertions | test |
| `tests/root_resolution.rs` (verify/edit) | assert the C1 pointer→`root` + `markdown_to_pdf_path` migration | test |
| `docs/*`, `AGENTS.md` (edit) | per spec §8 | — |

---

## C5.1 — Pure inverse + grouping (`conflicts.rs`)

### Task 1: `parse_conflict_name` — the strict inverse of `conflict_name`

**Files:** Edit `src/sync/conflicts.rs`.

The C2 builder is `conflict_name(original, host, date)` →
`stem (conflict <host> <date>).ext`. C5.1 recovers `{ original, host, date }` from
a friendly copy name. It must be strict: reject anything that isn't the exact
grammar (a note whose title merely contains the words "(conflict ...)" is not a
copy).

- [ ] **Step 1: RED — write failing round-trip + rejection tests**

```rust
/// Recovered parts of a friendly conflict-copy name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ParsedConflict {
    /// Canonical original this copy competes with, e.g. `notes/idea.md`.
    pub original: PathBuf,
    pub host: String,
    pub date: String,
}

/// Inverse of `conflict_name`: from `stem (conflict <host> <date>).ext` recover
/// the original path + host + date. `None` when `path`'s file name isn't the
/// exact friendly-conflict grammar.
#[must_use]
pub fn parse_conflict_name(path: &Path) -> Option<ParsedConflict> { /* … */ }
```

Tests (inline `#[cfg(test)]`):

```rust
#[test]
fn round_trips_conflict_name_for_a_matrix() {
    for (orig, host, date) in [
        ("notes/idea.md", "mac", "2026-07-25"),
        ("README", "server-01", "2026-01-02"),          // extensionless
        ("a/b c/my great note.md", "mac", "2026-12-31"), // spaces in stem + dir
        ("deep/nested/path/file.tar.gz", "mac", "2026-07-25"), // multi-dot ext
    ] {
        let built = conflict_name(Path::new(orig), host, date);
        let parsed = parse_conflict_name(&built).expect("should parse");
        assert_eq!(parsed.original, PathBuf::from(orig));
        assert_eq!(parsed.host, host);
        assert_eq!(parsed.date, date);
    }
}

#[test]
fn rejects_non_conflict_names() {
    assert!(parse_conflict_name(Path::new("notes/idea.md")).is_none());
    // A real title that happens to mention a conflict but isn't the grammar.
    assert!(parse_conflict_name(Path::new("notes/the (conflict) resolution.md")).is_none());
    // rclone's raw marker is not a friendly copy.
    assert!(parse_conflict_name(Path::new(&format!("idea.md.{CONFLICT_MARKER}1"))).is_none());
}
```

- [ ] **Step 2: GREEN.** Parse the file name: split the stem on the last
  ` (conflict ` … `)` group. `conflict_name` inserts ` (conflict <host> <date>)`
  immediately before `.ext` (or at the end when extensionless), so:
  - take `file_name`; find the **last** ` (conflict ` and the matching trailing
    `)`; everything before is `<stem>`, everything after the `)` is `.ext` (or
    empty); inside is `<host> <date>` split on the last space (date is the
    `YYYY-MM-DD` token, host is the rest — host is unqualified + trimmed by
    `command::hostname`, so it never contains ` (conflict ` or `)`).
  - reconstruct `original = dir.join(format!("{stem}{ext}"))`.
  - Validate `date` looks like `\d{4}-\d{2}-\d{2}` and `host` is non-empty; else
    `None`.
- [ ] **Step 3: REFACTOR** and note the grammar assumption in a comment. Keep
  `conflict_name` and `parse_conflict_name` adjacent so the inverse relationship is
  obvious. Run `cargo test --release conflicts::`.

**Verification:** the round-trip test passes for the full matrix; rejections hold; clippy clean.

### Task 2: `group_conflicts` + `copies_for_original`

**Files:** Edit `src/sync/conflicts.rs`.

- [ ] **Step 1: RED.**

```rust
/// A canonical original and its open conflict copies.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConflictGroup {
    pub original: PathBuf,
    pub copies: Vec<ParsedCopy>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ParsedCopy {
    pub path: PathBuf,   // relative to root
    pub host: String,
    pub date: String,
}

/// Fold flat `list_conflicts` output into groups keyed by recovered original.
/// Deterministic order (by original, then copy path). Copies that don't parse
/// are dropped (they aren't friendly copies).
#[must_use]
pub fn group_conflicts(files: &[ConflictFile]) -> Vec<ConflictGroup> { /* … */ }

/// The copies (from the live conflict set) belonging to `original`. Never the
/// original itself. `original` is matched as the recovered `ParsedConflict.original`.
#[must_use]
pub fn copies_for_original(original: &Path, files: &[ConflictFile]) -> Vec<PathBuf> { /* … */ }
```

Tests:

```rust
#[test]
fn groups_multiple_copies_of_one_original() {
    let files = vec![
        ConflictFile { path: "idea (conflict mac 2026-07-25).md".into() },
        ConflictFile { path: "idea (conflict server 2026-07-24).md".into() },
        ConflictFile { path: "other (conflict mac 2026-07-25).md".into() },
    ];
    let groups = group_conflicts(&files);
    assert_eq!(groups.len(), 2);
    let idea = groups.iter().find(|g| g.original == PathBuf::from("idea.md")).unwrap();
    assert_eq!(idea.copies.len(), 2);
}

#[test]
fn copies_for_original_returns_only_that_originals_copies() {
    let files = vec![
        ConflictFile { path: "idea (conflict mac 2026-07-25).md".into() },
        ConflictFile { path: "other (conflict mac 2026-07-25).md".into() },
    ];
    let got = copies_for_original(Path::new("idea.md"), &files);
    assert_eq!(got, vec![PathBuf::from("idea (conflict mac 2026-07-25).md")]);
    assert!(copies_for_original(Path::new("missing.md"), &files).is_empty());
}
```

- [ ] **Step 2: GREEN** using `parse_conflict_name`. Sort groups + copies for
  determinism (JSON stability). Also add `#[derive(serde::Serialize)]` to the
  existing `ConflictFile` if the JSON path needs it (Task 3 decides).
- [ ] **Step 3: REFACTOR**, run `cargo test --release conflicts::`, clippy.

---

## C5.2 — `brain sync conflicts --json`

### Task 3: the `--json` flag + grouped stdout output

**Files:** Edit `src/cli.rs`, `src/sync/command.rs`, `src/main.rs`.

- [ ] **Step 1: RED.** In `command.rs`, add a **pure** builder tested without IO:

```rust
/// Build the JSON value for `brain sync conflicts --json` from pre-collected
/// groups + per-copy metadata. Pure (metadata injected). Shape per spec §3.2.
#[must_use]
pub fn conflicts_json(groups: &[ConflictGroup], meta: &MetaLookup) -> serde_json::Value { /* … */ }
```

Test: an empty slice serializes to `[]`; a one-group slice serializes with keys
`original`, `original_exists`, `copies[]{path,host,date,modified,bytes}`; a copy
whose metadata is missing serializes `modified`/`bytes` as `null`.

- [ ] **Step 2: GREEN.**
  - `cli.rs`: `Conflicts { #[arg(long)] json: bool }`.
  - `command.rs`: `print_conflicts(root, json)`; when `json`, walk
    `group_conflicts(&list_conflicts(root))`, attach `fs::metadata` for each copy
    (mtime → RFC3339 via `chrono`, `len()` → bytes; failures → `null`) and
    `original.exists()`, then `println!("{}", serde_json::to_string_pretty(&value))`
    to **stdout**. When `!json`, today's themed line-list is unchanged.
  - `main.rs`: pass `args`' `json` through the `Conflicts` arm.
- [ ] **Step 3: REFACTOR.** Keep the impure metadata gather in a tiny helper; the
  shape logic stays in the tested `conflicts_json`. `cargo test --release`, clippy.

**Manual verification:** in a throwaway dir with a couple of friendly copies,
`brain sync conflicts --json` prints valid grouped JSON; `brain sync conflicts`
prints the old list.

---

## C5.3 — `brain sync resolve <original>`

### Task 4: the `Resolve` subcommand — safe local delete

**Files:** Edit `src/cli.rs`, `src/sync/command.rs`, `src/main.rs`.

- [ ] **Step 1: RED.** Test the pure decision + the guard through a tiny seam:

```rust
/// Result of resolving one original (pure classification of what to do).
pub enum ResolveDecision {
    /// Delete these copies (canonical exists).
    Delete(Vec<PathBuf>),
    /// Refuse: the canonical original is missing — merge into it first.
    CanonicalMissing,
    /// Nothing to do: no copies for this original.
    NoCopies,
}

#[must_use]
pub fn resolve_decision(original: &Path, canonical_exists: bool, files: &[ConflictFile])
    -> ResolveDecision { /* … */ }
```

Tests: canonical missing → `CanonicalMissing` (deletes nothing); canonical present
+ copies → `Delete([...])`; canonical present, no copies → `NoCopies`.

- [ ] **Step 2: GREEN.**
  - `cli.rs`: `Resolve { originals: Vec<String> }` (0..n; empty → interactive).
  - `command.rs`: `resolve(root, &originals)`:
    - for each original: `resolve_decision(orig, root.join(orig).exists(),
      &list_conflicts(root))`; on `Delete`, `fs::remove_file(root.join(copy))` for
      each; print a themed `resolved <original>: removed N copies`. On
      `CanonicalMissing`, print a themed warning and skip. Never runs `brain sync`.
    - bare (empty `originals`): interactive picker over `group_conflicts` (themed
      prompt listing each original + copy count; the human picks; agent path always
      passes originals explicitly). Keep the picker a thin shell; no unit test.
  - `main.rs`: dispatch the `Resolve` arm.
- [ ] **Step 3: REFACTOR**, `cargo test --release`, clippy.

**Manual verification:** with a merged canonical present, `brain sync resolve <path>`
removes only that path's copies; with the canonical deleted, it refuses and removes
nothing.

---

## C5.4 — The two generic skill rows

### Task 5: `cloud-sync` + `resolve-conflicts` in the bundled second-brain skill

**Files:** Edit `skills/second-brain/SKILL.md`.

- [ ] **Step 1: RED.** Add a bundled-skill assertion (in `src/skills/embed.rs`
  tests, beside `bundles_the_generic_triage_skill`) that the second-brain skill text
  contains the two new command anchors and the disambiguation callout:

```rust
#[test]
fn second_brain_bundles_the_cloud_sync_rows() {
    let text = /* second-brain SKILL.md text */;
    assert!(text.contains("/second-brain cloud-sync"));
    assert!(text.contains("/second-brain resolve-conflicts"));
    assert!(text.contains("brain sync conflicts --json"));
    assert!(text.contains("brain sync resolve"));
}
```

- [ ] **Step 2: GREEN.** Add the two rows per spec §5:
  - **`### Cloud-sync the brain / /second-brain cloud-sync`** — a callout that this
    is **different** from the existing `/second-brain sync` (lookup rebuild): this
    one syncs files across machines via `brain sync`. Steps: run `brain sync`
    (optionally `--push`/`--pull`), echo the summary; run `brain sync status`,
    surface the open-conflict count + needs-attention inline; if >0, nudge to
    `/second-brain resolve-conflicts`. If "sync is not configured", point at
    `brain sync setup` and stop. End with the additions table.
  - **`### Resolve sync conflicts / /second-brain resolve-conflicts`** — steps:
    `brain sync conflicts --json`; per group, read the canonical + each copy (host +
    `modified` recency), merge into the canonical, `brain sync resolve <original>`;
    after all groups, one `brain sync`; end with the additions table. Note it's
    scoped to prose keep-both copies (CSVs merge automatically; residual
    soft-conflicts show only in the journal).
  - Keep both **100% generic** — no bucket/host/email/org/private-path token. Run
    `cargo test --release bundled_skills_carry_no_personal_data` and the new anchor
    test.
- [ ] **Step 3: REFACTOR.** Run `brain skills sync` locally as a render sanity check
  (no error); confirm the guard test is green.

**Verification:** `cargo test --release` green including the guard + anchor tests.

---

## C5.5 — Migration: C1 verification + runbook + gated round-trip

### Task 6: prove the C1 root / `markdown_to_pdf_path` migration

**Files:** Verify `tests/root_resolution.rs`; add a test if missing.

- [ ] **Step 1.** Read `tests/root_resolution.rs` (and `src/env/`, `src/paths.rs`).
  If a test already seeds a throwaway `HOME`/`XDG_CONFIG_HOME`, writes a legacy
  `~/.config/brain-root` pointer, and asserts first-run resolution migrates it into
  `~/.config/brain/env.json`'s `root` key — **cite it** in the plan checkbox and
  move on. If not, **RED** a new test that does exactly that, then confirm GREEN
  (C1 already implements the behavior; this is a coverage test, so it should pass
  once written correctly — if it fails, that's a real C1 gap to flag, not patch
  here).
- [ ] **Step 2.** Likewise assert `markdown_to_pdf_path` resolves from
  `~/.config/brain/env.json` (moved out of brain config in C1). Sandbox every real
  path under the throwaway HOME/XDG; never touch the user's real `~/.config`.

### Task 7: the migration runbook

**Files:** Edit `docs/features.md` (sync section) — or `docs/migration.md` if the
section grows too large (open question §OQ).

- [ ] Document the per-machine steps (no live commands run by the agent): create the
  B2 bucket once; `brain sync setup` on each machine; verify triggers via
  `brain sync status`; the A→B verification checklist (edit/add/delete round-trips;
  CSVs merge with no conflict copy; one keep-both copy resolvable via
  `/second-brain resolve-conflicts`); track `~/.config/brain/env.json` privately if
  desired (brain stays agnostic). Keep it generic — a *runbook*, no personal bucket
  names.

### Task 8: gated local round-trip + resolve integration test

**Files:** Edit `tests/sync_local.rs`.

- [ ] **Step 1: RED/GREEN (gated).** Following the existing `sync_local.rs` pattern
  (PATH-gated on `rclone`, throwaway temp dirs, local backend, **no B2**), add a
  test that drives `sync_once`-equivalent flow between two local "machines" A and B
  through a shared remote dir and asserts:
  1. an **edit / add / delete** on A appears on B after a sync;
  2. `tasks.csv` / `habits.csv` given diverging edits **merge with no `(conflict …)`
     copy** appearing under either side (Lane B excludes them from bisync);
  3. a **concurrent prose edit** produces **exactly one** friendly keep-both copy,
     and the resolve path — `group_conflicts(&list_conflicts(root))` →
     `copies_for_original` → delete → assert only the canonical remains — cleans it
     up.
- [ ] **Step 2.** Keep the test `#[ignore]` or early-return when rclone is absent so
  the default suite stays green offline. Sandbox all paths under the throwaway HOME.

**Verification:** `cargo test --release` green (gated test skipped without rclone;
passing with it); manual `cargo test --release -- --ignored sync_local` when rclone
is present.

---

## C5.6 — Docs + contract row

### Task 9: documentation

**Files:** `docs/features.md`, `docs/integrations.md`, `docs/data-model.md`,
`docs/architecture.md`, `docs/decisions.md`, `AGENTS.md` (+ `CLAUDE.md` symlink).

- [ ] `docs/features.md` — `/second-brain cloud-sync` + `resolve-conflicts`;
  `brain sync conflicts --json`; `brain sync resolve`; the inline conflict nudge;
  the migration runbook (if here).
- [ ] `docs/integrations.md` — the structured enumerator + resolve deleter as the
  skill's brain-side contract.
- [ ] `docs/data-model.md` — the `conflicts --json` group schema
  (`ParsedConflict`/`ConflictGroup`/`ParsedCopy`).
- [ ] `docs/architecture.md` — the enumerator/resolver surface in `src/sync/`.
- [ ] `docs/decisions.md` — distinct `cloud-sync` name (no clobber of `sync`);
  structured-list + brain-deleter over pure prose; resolve's canonical-exists guard;
  resolve is a pure local delete (one final sync in the skill row); prose-only
  resolution scope for C5.
- [ ] `AGENTS.md` docs-contract table — a C5 row: "the second-brain sync skill rows
  (`cloud-sync`, `resolve-conflicts`), `brain sync conflicts --json`, and `brain
  sync resolve`" → `docs/features.md` + `docs/integrations.md` + `docs/data-model.md`
  (`parse_conflict_name`/`group_conflicts`/`copies_for_original` in
  `src/sync/conflicts.rs`; the bundled rows in `skills/second-brain/SKILL.md`).
- [ ] **Housekeeping:** `.difit/*` from the merged branches — **already removed**
  before the build; nothing to do.

**Verification:** docs agree with the shipped surface; `AGENTS.md` row present.

---

## Self-Review

Before the final adversarial review, confirm:

- [ ] `cargo test --release` green; `cargo clippy --release --all-targets` clean (pedantic + nursery, no new warnings).
- [ ] `parse_conflict_name` round-trips `conflict_name` for the full matrix and rejects non-grammar names (including the raw rclone marker and a title that merely contains "(conflict …)").
- [ ] `brain sync conflicts --json` emits the documented schema; `[]` when clean; `null` on metadata-read failure.
- [ ] `brain sync resolve` deletes only the named original's copies, never the canonical, and refuses when the canonical is missing.
- [ ] `resolve` runs no sync; the `resolve-conflicts` skill row ends with exactly one `brain sync`.
- [ ] Both skill rows are generic — `bundled_skills_carry_no_personal_data` green; the `/second-brain sync` (lookup rebuild) row is untouched.
- [ ] The gated local test proves edit/add/delete A→B, CSV merge with no conflict copy, and exactly-one-keep-both resolvable; it never points at B2 and skips cleanly without rclone.
- [ ] The C1 migration (pointer→`root`, `markdown_to_pdf_path`→env) is covered by a test under a throwaway HOME/XDG.
- [ ] Docs updated in the same change; the `AGENTS.md` contract row added.

## Open questions (resolve at build)

- **§OQ Runbook location.** `docs/features.md` sync section vs. a dedicated
  `docs/migration.md`. Lean features.md unless it bloats the section; decide at Task 7.
- **`ConflictFile` Serialize.** Whether the JSON path serializes `ConflictFile`
  directly or only the derived `ConflictGroup`/`ParsedCopy`. Lean: serialize only
  the grouped structs (richer, host/date attached); decide at Task 3.
- **Interactive picker reuse.** Whether `resolve`'s picker can reuse an existing
  themed-prompt helper (`src/personalization/checklist/` or the setup prompts) vs. a
  small local prompt. Prefer reuse if one fits; decide at Task 4.
