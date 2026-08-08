---
id: BR-4
title: Audit env ownership and promote portable values to brain config
status: backlog
priority: none
assignee: jpsyx
labels: [enhancement, tech-debt]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-08
updated: 2026-08-08
---

# BR-4: Audit env ownership and promote portable values to brain config

## Description

Review every value currently modeled as brain environment data and decide
whether it belongs in machine-local environment state, portable per-workspace
brain config, the workspace registry, or another explicit owner. The goal is to
make values that should travel with a workspace discoverable and portable,
while keeping secrets, machine paths, credentials, and runtime state out of
synced workspace data.

The review should produce an actionable migration and compatibility plan for
any values that move, including their CLI surfaces, documentation, tests, and
cross-machine behavior.

## Acceptance criteria

- [ ] Every env field is inventoried from its schema, store, migration code, documentation, and runtime consumers.
- [ ] Each field has an explicit ownership decision: machine-local env, portable brain config, structural workspace-registry data, secret/credential storage, or another documented owner, with rationale.
- [ ] Values proposed for brain config are checked for workspace scope, sync behavior, security, and multi-workspace isolation.
- [ ] The task identifies required schema, migration, CLI, documentation, and test changes, including backward-compatibility and recovery behavior.
- [ ] The final recommendation includes an implementation order and guardrails that prevent secrets, host-specific paths, and runtime state from becoming portable workspace data accidentally.

## Notes

### Pointers (as of 2026-08-08)

High-level guide to where and how to complete this, not a detailed plan
(references drift before the task is picked up).

- `src/env/schema.rs`, `src/env/store.rs`, `src/env/vars/`, and `src/env/migrate.rs` — current environment schema, persistence, variable access, and migration behavior to inventory.
- `src/settings/schema.rs`, `src/settings/store.rs`, `src/settings/vars.rs`, and `src/config.rs` — existing brain-config schema and typed configuration surfaces to compare against env ownership.
- `src/workspace/registry/` and `src/workspace/context.rs` — structural workspace identity, registry records, and selected-workspace context that should remain distinct from portable config.
- `docs/config.md`, `docs/data-model.md`, `docs/decisions.md`, and `docs/architecture.md` — documentation contract and existing ownership decisions to reconcile with the inventory.
- `tests/env_cli.rs`, `tests/workspace_runtime_isolation/`, `tests/workspace_registry_migration.rs`, `tests/sync_workspace_identity.rs`, and `tests/sync_local/multi_workspace.rs` — coverage for env behavior, isolation, registry migration, sync identity, and multi-workspace portability.

### Log

- 2026-08-08 created.
