# Brain Multi-Workspace Roadmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this roadmap. Each linked phase repeats its own required skills, tests, interfaces, and exit criteria.

**Goal:** Deliver the approved multi-workspace design in dependency order while keeping Brain usable and testable at every phase boundary.

**Architecture:** Five plans progressively establish immutable workspace context, portable users and task identity, the agent facade and advisory access policy, shared TUI-lifetime server routing, then workspace-scoped sync and coordinated migration. New schemas remain behind compatibility gates until the rollout phase activates them.

**Tech Stack:** Rust 2024, clap, serde, uuid, rusqlite, tiny_http, portable-pty, rclone, bundled Python scripts.

**Global Constraints:** The approved specification is `docs/superpowers/specs/2026-08-02-brain-multi-workspace-design.md`. Every behavior follows red/green TDD. Do not partially activate a schema or route before all of its writers/readers are workspace-aware. Do not claim hard tenant isolation. Preserve the user's data through backups and idempotent migration. Any implementation commit must follow the repository version-bump policy.

## Phase Order

1. **Foundation:** `docs/superpowers/plans/2026-08-02-brain-multi-workspace-foundation.md`
   - Adds registry schema version 2, selection, aliases, manifests, immutable `WorkspaceContext`, readiness, and UUID-scoped runtime paths.
   - Exit gate: no normal runtime command resolves a global root.

2. **Users, tasks, and triage:** `docs/superpowers/plans/2026-08-02-brain-multi-workspace-users-tasks-triage.md`
   - Adds portable users, actor precedence, assignment, immutable task UUIDs, collision reconciliation, and optional managed triage habits.
   - Build task/user schema migrators behind an inactive migration interface. Do not mutate real legacy workspaces until Phase 5 activates the coordinated gate.
   - Exit gate: all readers and writers understand the new schemas in fixtures, and legacy readers remain supported.

3. **Agent controller and advisory access:** `docs/superpowers/plans/2026-08-02-brain-agent-controller-access.md`
   - Extracts Claude/Codex behavior behind `AgentController`, adds workspace boundary prompts and capability policy, and stubs OpenCode.
   - Exit gate: TUI and receiver paths use semantic controller operations with no direct frontend branching.

4. **Shared server and receiver:** `docs/superpowers/plans/2026-08-02-brain-shared-server-receiver.md`
   - Adds leases, election, heartbeats, ingress routing, persistent receiver enablement, and final-TUI shutdown.
   - Exit gate: two workspace TUIs share one process, and no accepted message can outlive or bypass its target TUI.

5. **Sync and rollout:** `docs/superpowers/plans/2026-08-02-brain-multi-workspace-sync-rollout.md`
   - Scopes every sync artifact, validates local/remote UUIDs, activates the journaled migration, audits optional features, runs acceptance, updates all docs, and performs release verification.
   - Exit gate: the complete personal-plus-family scenario passes and the current public version is migrated safely.

## Cross-Phase Merge Rules

- Keep incomplete new behavior inaccessible from CLI dispatch until its phase exit gate passes.
- Land characterization tests before extracting current Claude, Codex, sync, receiver, or task behavior.
- Prefer additive readers before destructive writers: read legacy and new schemas first, migrate second, stop emitting legacy only after verification.
- A later phase may refine an earlier type only through a red test and a repository-wide compiler-driven update. It may not reintroduce global lookup helpers for convenience.
- If execution pauses between phases, the full release test suite and Clippy must be green, and real workspace files must remain on the last fully supported schema.

## Final Verification

Completion requires every phase exit criterion plus:

```sh
cargo test --release
cargo clippy --release --all-targets
python3 -m unittest discover -s skills/todo/scripts/tests
```

Run gated rclone/watch integration tests when their prerequisites exist. Review `README.md` and security/config docs to ensure workspace-only mode is described as easy-to-bypass advisory enforcement, never as a sandbox or security boundary.
