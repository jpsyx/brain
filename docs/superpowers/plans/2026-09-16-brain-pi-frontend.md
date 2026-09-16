# Brain pi frontend implementation plan

**Spec:** [../specs/2026-09-16-brain-pi-frontend-spec.md](../specs/2026-09-16-brain-pi-frontend-spec.md)
**Contract:** [../../adding-an-agent-frontend.md](../../adding-an-agent-frontend.md)

Every task is red first: write the smallest failing test, watch it fail, make it
pass, refactor with the bar green. Tasks are ordered so the tree compiles at the
end of each one.

---

### T1. Shared direct-invocation parsing

Extract `is_direct_claude_invocation` / `parse_direct_command` from
`src/agent/claude.rs` into `src/agent/direct_command.rs` as
`is_direct_invocation(command, executable, owned_flags)`. Claude keeps its
current behavior through the shared helper; pi reuses it in T8.

*Red:* the existing Claude evidence tests still pass against the shared helper,
plus a new test that a different executable name and flag set is honored.

### T2. `AgentKind::Pi`

`src/agent/session.rs`: add the variant, extend `ALL` to 4, add `"pi"`, and add
`AgentKind::parse_exact(&str) -> Option<Self>` built from `ALL` so every future
frontend is covered by construction.

*Red:* `all_frontends_are_listed_once_in_display_order` expects the 4-element
order; `parse_exact` round-trips every kind and rejects an unknown string.

### T3. State schemas and migration

- Widen the `agent_kind` CHECK list in `src/state/receiver/schema.rs`,
  `src/state/receiver/schema/delivery/cleanup_schema.rs`, and
  `src/state/manual_session/schema.rs`.
- New `src/startup_migration/pi_frontend.rs` (`up` and `down`) rebuilding
  `receiver_session_registrations` and `manual_sessions` when the stored SQL
  still carries the old list; `receiver_answer_cleanups` already self-repairs
  through `ensure_table_contract`.
- Point `parse_agent_kind` / `parse_frontend` at `AgentKind::parse_exact`.

*Red:* inserting a `pi` row into each table succeeds on a database created
before the change (migration test), and the migration's `down` restores the
previous contract.

### T4. The lifecycle bridge

`scripts/pi_brain_extension.ts`, modeled on `scripts/opencode_brain_plugin.js`:
env allowlist, `python3` spawn through `node:child_process`, never throws.

Events: `session_start`, `message_end` (track the turn's last non-error
assistant text), `agent_settled` (publish once), `input` (receiver acceptance
with a generated turn id), `tool_execution_end` (receiver progress).

`scripts/receiver_observation_bridge.py`: `accepted_turn_id` accepts `pi`.

*Red:* extend `tests/receiver_observation_bridge.rs` so a `pi` payload is
accepted, and add a Node-executed bridge test that asserts the payload the
extension hands each hook (following `tests/opencode_plugin.rs`).

### T5. Registry contract descriptors

`src/agent/registry/contract.rs`: `PI_LIFECYCLE` (one `StaticFile` at
`.brain/hooks/pi_brain_extension.ts`, mode `0644`) and `PI_HEALTH` (that file's
exact contents plus the three shared bridges).

*Red:* lifecycle ids stay globally unique; doctor reports the new checks.

### T6. Session discovery

`src/agent/pi/sessions.rs`: pure `encoded_directory_name(cwd)`,
`session_dir(home, env overrides, cwd)`, and `rollout_matches(file_name, id)`,
plus the IO wrapper.

*Red:* the encoding matches pi's own transform, and a
`<timestamp>_<id>.jsonl` file is found while a same-prefix different-id file is
not.

### T7. The probe

`src/agent/pi/probe.rs` + `src/agent/pi/probe/runner.rs`: version floor
`0.84.1`, required `--help` flags, and the `--list-models` readiness check
(real environment, `PI_OFFLINE=1`, read-only), each with its own actionable
error; successful results cached per command.

*Red:* fake `pi` scripts covering: unavailable, old version, malformed version,
a help output missing one flag, an empty model catalog, and a healthy install.

### T8. The adapter

`src/agent/pi.rs`: `PiFrontend` with `command_for` (base:
`--no-approve --session-id <id> [-- <prompt>]`) and a private builder adding
skills, boundary prompt, and `-e <extension>`; `input_for`; `completion_strategy
= Hook`; resume through T6; `response_id` as a UUIDv5 of
`brain://pi/response/<id>`; capability evidence through T1 and T9.

*Red:* a new row in `src/agent/adapter_tests/contract.rs::frontend_contracts()`
runs the entire shared adapter suite for pi.

### T9. Honest capability evidence

`src/access/enforcement.rs`: add `strict_skills` evidence and an
`mcps_unsupported` fact with `EnforcementEvidence::without_mcp_support()`;
`CapabilityPlan::enforcement_report` maps an unsupported mechanism to
`Unavailable` for every requested name.

*Red:* `brain skills status` reports pi MCP rows as `unavailable` and pi skill
rows as `strictly-selected` for a direct `pi` command.

### T10. Registration and selection surfaces

`src/agent/registry.rs` (4th registration), `src/agent/mod.rs` re-exports,
`src/agent/frontend.rs` ambient `PI_*` environment, `src/cli/global.rs`
(`--pi` / `-pi`), `src/agent/default_frontend.rs` (`parse`), `src/env/schema.rs`
(`pi_cmd`, and `default_agent_frontend`'s description).

*Red:* selector parsing in every position, conflict rejection, `parse("pi")`,
`brain env list` showing `pi_cmd`.

### T11. Doctor

Make `format_doctor_plan` and the doctor log line registry-driven instead of
naming Claude and OpenCode literally, so the pi row appears without a fourth
hard-coded string.

*Red:* the plan names every frontend that declares a compatibility probe.

### T12. Docs and version

`docs/README.md` (read order and source map), `architecture.md`, `features.md`,
`integrations.md`, `config.md`, `data-model.md`, `decisions.md`, `testing.md`,
`glossary.md`, and `AGENTS.md`'s docs-contract row for frontends. Bump
`Cargo.toml` to `0.91.0` and move `Cargo.lock`.

### T13. Acceptance

`cargo test --release`, `cargo clippy --release --all-targets -- -D warnings`,
then a real-binary check: `brain tasks doctor` reporting pi ready, the bridge
installed in the selected workspace, and one interactive `brain --pi` session
that starts, answers, records its session id, and resumes.

---

## What shipped, and where it differed from the plan

- **T3 went further than "widen three lists".** The `agent_kind` `CHECK` list is
  now generated from the frontend registry (`src/state/frontend_contract.rs`),
  and any table whose stored definition has drifted is rebuilt when the database
  is opened. The next frontend needs no schema edit and no new migration. The
  0.91.0 migration's `down` reuses the same rebuild to restore the pre-pi list
  and drop the rows only pi could own.
- **T9's constructors** are `EnforcementEvidence::without_mcp_support()` and
  `strict_skills_without_mcp_support()`.
- **Two test surfaces were added beyond the plan.** `tests/pi_acceptance.rs`
  drives a real fake pi process through the facade (argv, terminal bytes,
  environment), and `tests/fixtures/pi/fake_pi.sh` stands in for pi everywhere a
  test would otherwise depend on the developer's own install, configuration, or
  provider credentials.
- **T13's live-session check is the one item left to a human.** Everything below
  a real LLM turn is covered automatically: `cargo test --release` and
  `cargo clippy --release --all-targets -- -D warnings` are clean, and
  `brain tasks doctor` reports the pi bridge installed and pi compatible against
  the real binary (pi 0.85.1).
