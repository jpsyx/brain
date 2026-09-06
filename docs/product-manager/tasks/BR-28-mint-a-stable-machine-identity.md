---
id: BR-28
title: Mint a stable per-machine identity
status: todo
priority: high
assignee: jpsyx
labels: [feature, sync]
estimate: 5
project: PROJ-2
milestone: MS-8
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-28: Mint a stable per-machine identity

## Description

Brain cannot answer "whose change was this", structurally. There is no machine
identity anywhere in the codebase: `grep -rn "machine_id\|device_id\|install_id"
src/` returns nothing. The only "who" is `sync::command::hostname()`, which
reads `$HOSTNAME` then `hostname(1)`, and on macOS that drifts with the network.
One laptop has filed conflict copies under three different names:

```
~/brain/.brain/hooks/
  agent_session_start_hook (conflict Mac 2026-08-25).py
  agent_session_start_hook (conflict MacBook-Pro-10 2026-08-31).py
  agent_session_start_hook (conflict Avandar-MacBook-Pro 2026-09-05).py
```

`docs/decisions.md` already concedes the consequence for the task CSVs: *"The
CSV lane has no per-machine author field."* `brain check` therefore has to say
"the remote differs from this machine's baseline" where a user reads "another
machine changed this".

A durable per-machine UUID is cheap and unblocks three things: honest provenance
in the journal and BR-20's ledger, single-writer-per-object state in BR-27, and
the cheap remote-freshness probe in BR-30's neighborhood.

Constraints found during review, each of which is a real hazard:

- **Location and mutability.** It belongs in `~/.config/brain/env.json` as a
  **structural, non-writable** field alongside `root`, not a declared `VarSpec`.
  Otherwise `brain env set` can rewrite machine identity.
- **Cloning duplicates it.** Migration Assistant, a Time Machine restore, or
  `rsync ~/.config` gives two machines the same id, which is strictly worse than
  hostname drift because nothing can detect it. BR-21 decides whether to bind
  the mint to a hardware fingerprint (`IOPlatformUUID`) and re-mint on change.
- **`down` must not re-mint.** The repo requires every migration to provide up
  and down. If `down` deletes the id, a downgrade-then-upgrade cycle mints a
  different one and orphans every durable reference. Either make `down` a no-op
  that leaves the id, or mint lazily on first read and skip the migration.
- **Keep the UUID out of the conflict filename.** `docs/data-model.md`
  specifies `name (conflict <host> <date>).ext` so resolving one is a normal
  file-manager task. A 36-character UUID destroys that, and
  `parse_conflict_name`'s `rsplit_once(' ')` would accept it silently, so the
  regression would be quiet. Keep the short hostname in the filename; the stable
  id goes in the journal, the ledger, and any state-file name.

## Acceptance criteria

- [ ] A machine UUID is minted exactly once per machine and persisted in `~/.config/brain/env.json` as structural, non-writable data; `brain env set` cannot change it, asserted by a test.
- [ ] The mint is idempotent: repeated invocations return the same id, and a concurrent first run on two processes cannot produce two ids.
- [ ] The clone hazard is handled per BR-21's decision. If fingerprint-binding is chosen, a changed fingerprint re-mints and the change is logged; if not, the hazard is documented in `docs/config.md` as a known limitation.
- [ ] If a startup migration is used, it provides both `up` and `down`, and `down` does not destroy the id. If lazy minting is used instead, no migration is added and that choice is recorded.
- [ ] The id is surfaced where a human can read it (`brain env list` or `brain tasks doctor`), because an identity nobody can see is undiagnosable.
- [ ] The conflict-copy filename grammar is unchanged, and `parse_conflict_name` still round-trips every existing name in the author's workspace.
- [ ] The sync journal records the machine id per run, so provenance is answerable retrospectively.
- [ ] `docs/config.md` documents the field as structural registry data, and `docs/data-model.md` plus `docs/decisions.md` record why hostname was insufficient.

## Notes

Sequence before BR-27 if single-writer-per-object state is chosen there, since
that design names files after the machine id. Sequence before BR-20's ledger for
its `winner_machine` and `loser_machine` columns, which are currently specified
as nullable precisely because this does not exist yet.

### Pointers (as of 2026-09-05)

- `src/env/schema.rs` — `is_structural` and the `VarSpec` table. The distinction between structural registry data and writable free-form env is exactly the one this task turns on; `root` is the existing precedent.
- `src/workspace/registry/` — the registry store, its transactional replace, and `migrate.rs` (including `backup_legacy`, which is the only place `env.json` is copied). Confirm nothing syncs or backs up this file elsewhere before calling it machine-local.
- `src/sync/command/mod.rs` — `hostname()`, the current stand-in and its three call sites. Leave it in the filename path; add the id alongside.
- `src/sync/conflicts/mod.rs` — `conflict_name` and `parse_conflict_name`, whose `rsplit_once(' ')` is why a UUID in the filename fails silently rather than loudly.
- `src/startup_migration/` — the up/down contract and the reconciliation pattern, if a migration is used. Read `mod.rs` for how existing migrations declare both directions.
- `src/sync/journal.rs` — the `sync_runs` schema; adding a column here means an open-and-migrate step in the same style.
- `docs/decisions.md`, search `## C3 —` for the "no per-machine author field" concession this task retires, and `## C1` / the brain-env split for why structural data is not writable.

### Log

- 2026-09-05 created from the investigation.
