# SDD ledger for plan: docs/superpowers/plans/2026-08-02-brain-multi-workspace-sync-rollout.md

Base: f28598469e58fc942de9a80a6fe3e292c4ea31e2
Starting version: 0.31.10
Branch: feat/multi-workspace-sync-rollout
Worktree: /Users/juanpablosarmiento/src/worktrees/brain/feat/multi-workspace-sync-rollout

Preflight: complete. See `preflight-audit.md`.

Preflight ruling: current code is authoritative. Preserve `WorkspacePaths` as the sole UUID-derived runtime path authority; do not add a redundant `SyncRuntime`. Manifest schema 1 remains current. Remote manifestless nonempty adoption is one shared Task 2/4 contract and fails closed until Task 4 adds explicit adoption. Receiver freshness remains owned by the exact live TUI. Preserve the documented markdown-to-PDF prerequisite during Task 6.

Execution ruling: the user's explicit workflow overrides SDD's per-task independent-review step and workspace-deletion finish step. Each implementer must self-review and pass task-local gates; no independent reviewer runs between tasks. One highest-capability independent reviewer runs only after all implementation and full gates. Preserve the branch and worktree.

Baseline gate: `cargo test --release` passed at f285984 (1271 unit tests plus all integration/doc tests; no failures).

Task 1: RED `cargo test --release --test sync_local local_rclone_populates_the_uuid_scoped_production_workdir -- --exact --nocapture` failed exit 101 because the harness bypassed the UUID-derived production workdir.
Task 1: minor (resolved in Task 7): pre-existing macOS receiver test cleanup could return ENOTCONN when the peer closed before `shutdown(Both)`. A final Task 7 release run reproduced it; cleanup now accepts only the already-disconnected terminal state, with no sleep or production change. The exact test passed 20/20, the receiver integration passed 21/21, and the full release suite passed.
Task 1: complete (commits f285984..64a70f8, self-review and task-local gates clean; version 0.31.11).

Task 2: RED `cargo test --release --test sync_workspace_identity` failed exit 101 on missing `brain::sync::identity`; compiled entry-point RED then proved sync/repair/check accepted a mismatched remote before the gate.
Task 2: complete (commits 64a70f8..f617e15, self-review and task-local gates clean; version 0.31.12). Shared `VerifiedRemote` gate now precedes all remote data lanes; setup initializes only a successfully listed empty target and verifies exact manifest bytes by read-back before credential persistence/baseline.

Task 3: RED compiled child bootstrap accepted a mismatched expected UUID, and subsequent compile REDs proved detached runner/lock/freshness/watcher injection seams were absent.
Task 3: complete (commits f617e15..e25bb45, self-review and task-local gates clean; version 0.31.13). Detached children carry canonical selector plus `BRAIN_WORKSPACE_ID`; bootstrap fails closed; concurrency, receiver freshness, and watcher lifecycle use injected clocks/runners and bounded polling with no fixed sleeps.

Task 4: RED setup CLI lacked `adopt_workspace_id`; subsequent REDs proved no exact UUID authorization, identity summary, interactive decision, or unreadable-manifest refusal existed.
Task 4: complete (commits e25bb45..dfa17eb, self-review and task-local gates clean; version 0.32.0). Nonempty manifestless adoption requires `--adopt-workspace-id <exact-selected-uuid>` or explicit interactive `y/yes`; `--yes` is rejected; exact manifest publication/read-back precedes every other write; schema 1 compatibility remains authoritative.

Task 5: RED coordinator module was absent; inactive task migration downgraded schema 3 to 2; journal/backup/CLI/recovery seams were absent; strengthened compiled preflight RED exposed access-mode seeding before mapping refusal.
Task 5: complete (commits dfa17eb..67e3ac2, self-review and task-local gates clean; version 0.33.0). `brain workspace migrate` is UUID-scoped, compatibility/remote/mapping/ack gated, final-legacy-sync-first, backed up, atomic, resumable, idempotent, fully verified, and ordinary startup/sync cannot activate it. Strict Clippy and full release suite green.

Task 6: RED began with missing centralized requirement types, then exposed partial sync being collapsed to off, disabled-receiver leakage, malformed typed receiver fields being treated as off, status/list/doctor filesystem writes, SQLite WAL mutation from ordinary read-only opens, missing Codex doctor parity, bundled core skills being misclassified as optional, and a read-only detached child bypassing expected-workspace UUID validation.
Task 6: complete (commits 67e3ac2..f4f69a8, self-review and task-local gates clean; version 0.34.0). Required availability is distinct from optional off/ready/incomplete health; every inspector reloads and UUID-checks only the selected workspace; status/list/doctor are literal no-write and redacted; Claude/Codex doctor health is equivalent; startup still owns the PDF prerequisite. Strict Clippy and full release suite green.

Task 7: RED composed acceptance first resolved the authenticated family sender as `family-member` instead of portable `wife`; the next run reached the real task script and exposed missing temporary `tasks/` provisioning. The two-workspace local-rclone RED then showed the old harness helper forced one fixed workspace ID, so the family workdir did not exist.
Task 7: final release RED reproduced Task 1's deferred macOS receiver cleanup race when a peer close made test-side `shutdown(Both)` return `NotConnected`; cleanup now accepts only that already-disconnected state, exact test 20/20 and receiver suite 21/21 green, with no sleep or production change.
Task 7: complete (single local task commit after f4f69a8, self-review and task-local gates clean; version 0.34.1). One hermetic scenario proves all eleven personal-plus-family lifecycle assertions through real Brain seams with fake external transports. The gated local-rclone complement proves concurrent UUID-scoped workdirs/baselines and pre-bisync mismatch refusal; watcher lifecycle uses bounded channels. Strict Clippy and full release suite green. Deferred minor: repo-wide rustfmt drift inherited from earlier branch tasks remains for Task 8, while every Task 7 Rust file passes focused rustfmt check.

Task 8: RED found the pre-release version, legacy docs-security assertions, stale `Alt-?` root help, and stale single-root/Claude Cargo description. Focused assertions failed before each corresponding release-surface change and then passed.
Task 8: complete in the local task commit after af1c502 (final version 0.35.0). All required durable docs, AGENTS contracts, CLI help, Cargo metadata, and release surfaces now match the Phase 5 module tree and explicit migration/remote identity/requirements/acceptance behavior. Full release, strict Clippy, personal-data, Python skill, local-rclone, watch, migration, acceptance, privacy, read-only, docs, help, temporary CLI smoke, stale-language, and patch-hygiene gates are green.
Task 8: deferred minor: exact base f285984 already has 1,310 repo-wide rustfmt diff lines/100 headers. Focused edition-2024 `skip_children=true` formatting over 102 Phase 5/Task 8 files passes; final repo-wide audit has 1,003 lines/76 headers and zero current-only path-normalized drift versus the exact base. No unrelated formatting sweep was retained.
