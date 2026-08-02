# Brain Shared Server and Receiver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan. Use rust-skills, test-driven-development, and systematic-debugging for any lifecycle race or flaky test.

**Goal:** Replace the independent background web daemon and TUI-owned receiver listener with one machine-wide, TUI-lifetime server that safely routes several live workspaces without sharing workspace state.

**Architecture:** A detached shared process owns the single loopback HTTP listener. Live TUIs register renewable workspace leases over a machine-global control socket and expose a workspace-local job socket. HTTP routes carry a portable opaque ingress ID; the server resolves the workspace before authentication and sender mapping, then forwards an accepted in-memory job to that workspace's TUI. The process exits immediately after an orderly final unregister, or after the final crashed lease expires.

**Tech Stack:** Rust 2024, tiny_http, Unix domain sockets, serde/serde_json, std threads/channels, existing HMAC/provider code.

**Global Constraints:** Complete the workspace, users, and agent-controller plans first. Never accept work without both receiver enablement and a live TUI lease. Never add durable inbound queues, replay, headless agent execution, manual server start/kill/restart, or an always-on availability responder. One workspace's route must never read another workspace's env, users, root, agent, response files, or logs. Any implementation commit must include the required version bump.

---

### Task 1: Model leases and routing as a pure state machine

**Files:**

- Replace: `src/server/lifecycle.rs` with `src/server/lifecycle/mod.rs`
- Create: `src/server/lifecycle/lease.rs`
- Create: `src/server/lifecycle/table.rs`
- Create: `src/server/lifecycle/decision.rs`
- Modify: `src/server/mod.rs`

- [ ] **RED: test lease registration and final-shutdown decisions**

Use an injected monotonic timestamp. Required tests:

```rust
let mut table = LeaseTable::default();
table.apply(register(personal(), lease_a(), now)).unwrap();
table.apply(register(family(), lease_b(), now)).unwrap();
assert_eq!(table.live_workspaces(now), [family_id(), personal_id()]);

assert_eq!(table.apply(unregister(lease_a(), now)), ServerDecision::KeepRunning);
assert_eq!(table.apply(unregister(lease_b(), now)), ServerDecision::ShutdownNow);
```

Also prove:

- Two leases for the same workspace are rejected even if lease IDs differ.
- Different workspaces coexist.
- A heartbeat renews only its lease.
- An expired lease is unavailable and removed.
- Expiring the final lease returns `ShutdownNow`.
- Re-registering after server recovery replaces only a stale same-workspace lease.
- Ingress IDs cannot collide across live registrations.

- [ ] Run `cargo test --release server::lifecycle::` and observe missing modules.

- [ ] **GREEN: implement the pure types**

```rust
pub struct LeaseId(Uuid);

pub struct WorkspaceLease {
    pub lease_id: LeaseId,
    pub workspace_id: WorkspaceId,
    pub canonical_name: WorkspaceName,
    pub ingress_id: IngressId,
    pub tui_pid: u32,
    pub job_socket: PathBuf,
    pub receiver_enabled: bool,
    pub expires_at: Instant,
}

pub enum ServerDecision { KeepRunning, ShutdownNow }
pub enum WorkspaceAvailability { Accepting(WorkspaceLease), Disabled, NoLiveTui, Unknown }
```

`LeaseTable::availability(ingress_id, now)` must distinguish disabled from no live TUI because both produce an unavailable provider reply while another workspace keeps the process alive. It must never return a stale lease.

- [ ] Use a short production heartbeat interval and a larger TTL constant, but inject both in tests. No test sleeps.

- [ ] Run focused tests and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 2: Implement shared-process election, state, and automatic shutdown

**Files:**

- Create: `src/server/lifecycle/paths.rs`
- Create: `src/server/lifecycle/state.rs`
- Create: `src/server/lifecycle/election.rs`
- Create: `src/server/lifecycle/process.rs`
- Create: `src/server/lifecycle/watchdog.rs`
- Modify: `src/server/mod.rs`
- Modify: `src/cli/server.rs`
- Modify: `src/command/server.rs`
- Modify: `src/command/dispatch.rs`
- Create: `tests/server_lifecycle.rs`

- [ ] **RED: characterize process decisions with injected probes**

Test `decide_start(record, pid_alive, socket_reachable, election_lock)` for reuse, stale-state cleanup, one elected starter, and losing contenders waiting for the winner. Test that the hidden server loop refuses startup without an election token.

- [ ] **RED: add a subprocess integration test for final shutdown**

Start the test server under a temporary cache root, register two leases, unregister one, assert it remains reachable, unregister the second, and assert the process exits and removes its PID/socket record. Use bounded polling, not fixed sleeps.

- [ ] Run the lifecycle integration test and observe current always-on behavior.

- [ ] **GREEN: move global infrastructure under one directory**

```text
~/.cache/brain/server/
├── process.json
├── control.sock
├── election.lock
└── server.log
```

`process.json` contains only PID, port, generation UUID, and start time. It contains no roots, users, senders, credentials, prompts, or message bodies.

- [ ] Replace `ensure_running()` with `connect_or_elect(&ServerClient)`. Only TUI startup and TUI crash-recovery may call it. `brain habits` and triage tabs use the already attached client; they do not create a server independently.

- [ ] Keep a hidden `brain server run --generation <uuid> --port <port>` implementation detail. Remove public `start` and `kill` enum variants and handlers. Add `logs`; keep `status` read-only.

- [ ] The server loop exits on `ShutdownNow`. The watchdog periodically expires crashed leases and exits after the final expiry. `Drop`/signal cleanup removes the process record only if its generation still matches.

- [ ] Run lifecycle tests repeatedly, full tests, and Clippy.

- [ ] Commit only if authorized; include the required version bump.

### Task 3: Add control protocol, heartbeats, and TUI registration

**Files:**

- Replace: `src/server/receiver/control.rs` with `src/server/control/mod.rs`
- Create: `src/server/control/protocol.rs`
- Create: `src/server/control/client.rs`
- Create: `src/server/control/server.rs`
- Create: `src/server/control/codec.rs`
- Modify: `src/server/mod.rs`
- Modify: `src/tui/event_loop/setup.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/singleton.rs`
- Create: `tests/server_control.rs`

- [ ] **RED: test protocol round trips and stale generations**

```rust
let message = ControlRequest::Register(registration_fixture());
assert_eq!(decode(encode(&message).unwrap()).unwrap(), message);
assert!(matches!(
    apply(ControlRequest::Heartbeat { generation: old_generation(), .. }),
    ControlResponse::StaleGeneration
));
```

Test register, heartbeat, update-enabled, unregister, snapshot/status, malformed frames, oversized frames, duplicate workspace, and stale process generation.

- [ ] Run focused tests and observe missing control server.

- [ ] **GREEN: implement newline-delimited JSON on the Unix socket**

Bound frame size and request time. The server validates each registration by reopening the machine registry and portable manifest, matching canonical name, workspace UUID, ingress ID, and root. Do not trust a root supplied by a client.

- [ ] TUI startup order becomes:

1. Resolve and ready the workspace.
2. Acquire the UUID-scoped TUI lock.
3. Bind the workspace-local job socket.
4. Connect to or elect the shared server.
5. Register the lease.
6. Start the heartbeat worker.
7. Launch the agent panel and event loop.

- [ ] TUI shutdown unregisters before removing its job socket. If unregister cannot reach the server, normal process exit still ends the TUI; the server watchdog expires it. Do not wait indefinitely.

- [ ] Every TUI detects a missing/stale server generation during heartbeat, races through the election lock, reconnects, and re-registers. Only one shared child may win.

- [ ] Run control integration tests with two fake TUI clients and one deliberate server crash.

- [ ] Commit only if authorized; include the required version bump.

### Task 4: Route every web endpoint by opaque workspace ingress ID

**Files:**

- Modify: `src/server/router.rs`
- Modify: `src/server/mod.rs`
- Modify: `src/server/routes/habits/mod.rs`
- Modify: `src/server/routes/habits/model.rs`
- Modify: `src/server/routes/triage/mod.rs`
- Modify: `src/triage_signal.rs`
- Create: `src/server/workspace_route.rs`
- Create: `tests/server_workspace_routing.rs`

- [ ] **RED: replace global-route tests with ingress-aware routing**

```rust
assert_eq!(
    route("POST", "/w/abc123/sms"),
    Route::Sms { ingress: IngressId::parse("abc123").unwrap() }
);
assert_eq!(route("GET", "/habits"), Route::NotFound);
assert_eq!(route("POST", "/sms"), Route::NotFound);
```

Cover `/w/<ingress>/habits`, `/habits/done`, `/triage/done`, `/sms`, and `/email`; malformed, missing, or extra path components; query stripping; and unknown ingress.

- [ ] Run router tests and observe failures.

- [ ] **GREEN: select workspace before route behavior**

`WorkspaceRouteResolver` maps ingress ID to a live lease, then reloads a verified `WorkspaceContext`. Handler signatures take that context explicitly. Delete all `paths::brain_root_path()` calls from server code.

- [ ] Local habits URLs and triage completion URLs are built from the selected workspace's ingress ID. Browser pages for two simultaneous workspaces read and mutate only their own task files.

- [ ] An unknown ingress returns 404 and no provider response. A known ingress with disabled/no-live-TUI availability follows Task 5's unavailable behavior for receiver routes; local web routes return 503.

- [ ] Run routing and two-workspace habits integration tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 5: Authenticate, resolve actor, and forward accepted jobs without a queue

**Files:**

- Replace: `src/server/receiver.rs` with `src/server/receiver/mod.rs`
- Create: `src/server/receiver/job.rs`
- Create: `src/server/receiver/dispatch.rs`
- Create: `src/server/receiver/unavailable.rs`
- Modify: `src/server/receiver/http/mod.rs`
- Modify: `src/server/receiver/http/sms.rs`
- Modify: `src/server/receiver/http/email.rs`
- Modify: `src/server/security.rs`
- Modify: `src/server/delivery.rs`
- Modify: `src/server/reply.rs`
- Modify: `src/tui/receiver_state.rs`
- Modify: `src/tui/app_brain.rs`
- Create: `tests/receiver_workspace_isolation.rs`

- [ ] **RED: test the ordered decision pipeline**

The pure decision sequence is:

```text
parse ingress -> resolve workspace availability -> load selected provider config
-> verify provider signature -> resolve sender to ActorContext
-> forward one in-memory job to the live TUI socket
```

Add tests proving provider credentials are loaded only after workspace selection, an identical sender can map to different users in different workspaces, unknown sender is rejected, and a personal route can never forward to the family socket.

- [ ] **RED: test unavailable and discard semantics**

When another TUI keeps the process alive:

- Disabled receiver sends one concise provider response and does not forward.
- Missing/expired target TUI sends one concise provider response and does not forward.
- Failed job-socket connection sends unavailable and does not store/retry.
- Accepted job exists only in the receiving TUI's in-memory queue.

When the shared process is absent, no test helper starts it and no response is possible. Do not add an availability-only process.

- [ ] Run isolation tests and observe current global receiver behavior.

- [ ] **GREEN: implement `InboundJob`**

```rust
pub struct InboundJob {
    pub job_id: Uuid,
    pub workspace_id: WorkspaceId,
    pub actor: ActorContext,
    pub channel: Channel,
    pub authenticated_sender: String,
    pub prompt: String,
    pub attachments: Vec<AttachmentRef>,
    pub received_at_unix_ms: u64,
}
```

The job socket accepts one bounded serialized job and acknowledges only successful enqueue into the live TUI's memory. It writes no spool, DB queue, replay file, or detached command.

- [ ] Keep message bodies out of the shared server log. Workspace-specific logs may include job IDs and delivery state, but not credentials. Preserve current authenticated response-delivery restrictions.

For email delivery, resolve the initiating actor's portable `response_email` at acceptance time and carry the normalized value in trusted job metadata. Delivery may target only that address, or an allowlisted participant already in the authenticated inbound thread. It may not substitute the machine local user's address or a response address from another workspace.

- [ ] Route the job's actor through `AgentController`; completion and delivery retain the same actor/channel even if local config changes during the turn.

- [ ] Run receiver, security, actor, agent, and integration tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 6: Persist receiver enablement and keep CLI/palette parity

**Files:**

- Modify: `src/cli/global.rs`
- Modify: `src/cli/server.rs`
- Modify: `src/command/server.rs`
- Modify: `src/command/dispatch.rs`
- Modify: `src/workspace/registry/model.rs`
- Modify: `src/workspace/registry/store.rs`
- Modify: `src/tui/modal_state.rs`
- Modify: `src/tui/palette/command.rs`
- Modify: `src/tui/palette/state.rs`
- Modify: `src/tui/app_actions/commands.rs`
- Modify: `src/menu/model.rs`
- Modify: `src/tasks/shortcuts.rs`
- Modify: `src/tui/draw_help.rs`
- Create: `tests/receiver_enablement.rs`

- [ ] **RED: test one shared mutation decision for every surface**

```rust
assert_eq!(receiver_transition(false, ReceiverAction::Start), true);
assert_eq!(receiver_transition(true, ReceiverAction::Stop), false);
assert_eq!(receiver_transition(false, ReceiverAction::WithReceiverFlag), true);
```

Prove CLI start/stop, `--with-receiver`, and palette toggle all persist the selected record's `receiver_enabled` and notify a live lease if one exists. They must not change another record.

- [ ] **RED: test the reduced command grammar**

`brain server status|logs` and `brain receiver setup|set|start|stop|status|logs` parse. Server start/kill and every restart spelling fail clap parsing. `--with-receiver -b family` enables family before TUI registration.

- [ ] Run focused CLI and transition tests and observe failures.

- [ ] **GREEN: remove obsolete commands and state**

Delete `ServerAction::{Start, Kill}`, `ReceiverServerAction::Restart`, manual handlers, help text, and palette restart rows. Keep hidden internal server-run dispatch inaccessible from normal help.

- [ ] Status output separates persistent intent from current availability:

```text
Receiver  enabled
TUI       live
Server    running
Accepting yes
```

If enabled without a TUI, show `Accepting no`; do not start the server. Server status shows process/lease counts only, without workspace message data.

- [ ] Update palette labels dynamically to `Enable receiver` or `Disable receiver`, driven by the same `App` state and pure transition. Update all required palette, shortcut, help, and keybinding surfaces even if the action has no direct key.

- [ ] Run CLI, palette, receiver, and full tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 7: Make receiver setup and URLs workspace-specific

**Files:**

- Modify: `src/command/server.rs`
- Modify: `src/server/receiver/http/mod.rs`
- Modify: `src/server/security.rs`
- Modify: `src/env/schema.rs`
- Modify: `src/env/vars.rs`
- Modify: `src/workspace/manifest.rs`
- Create: `tests/receiver_setup_workspace.rs`

- [ ] **RED: test setup plans without touching providers**

Prove setup writes provider secrets and public base URL only to the selected machine registry record, stores the stable ingress ID only in portable `workspace.json`, and renders exact webhook URLs containing that ingress ID.

- [ ] Add a second-workspace fixture proving Twilio/Resend credentials can differ by workspace on one machine and no setup output leaks secret values.

- [ ] Run setup tests and observe current global env behavior.

- [ ] **GREEN: pass `WorkspaceContext` into every setup/set helper**

`receiver_webhook_url(public_base_url, ingress_id, channel)` emits `/w/<ingress>/<channel>`. Setup validates a portable ingress ID exists, creates it only for a newly initialized workspace, and never rotates it during rename, alias, default change, or machine attach.

- [ ] User mapping is edited through `users.json`: adding a phone/email asks for an existing user or creates one. Phone fields are required only for configured SMS; email fields are required only for configured email. Non-interactive flags must cover user ID, display name, address, and allowed state.

- [ ] Configuration changes notify the live shared server to reload the selected workspace. No restart command or process churn.

- [ ] Run setup, users, security, and full tests.

- [ ] Commit only if authorized; include the required version bump.

### Task 8: Document and exercise the complete lifecycle

**Files:**

- Modify: `README.md`
- Modify: `docs/glossary.md`
- Modify: `docs/architecture.md`
- Modify: `docs/features.md`
- Modify: `docs/config.md`
- Modify: `docs/integrations.md`
- Modify: `docs/data-model.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `docs/keybindings.md`

- [ ] Document leases, election, heartbeats, final shutdown, crash recovery, ingress routing, enablement versus process state, unavailable behavior, and the deliberate absence of offline queues.

- [ ] State plainly: if every TUI is closed, no server exists and an inbound text receives no Brain response. If another TUI keeps the server alive but the target workspace is unavailable, the sender gets one unavailable response and the message is discarded.

- [ ] Add an end-to-end test harness that launches personal and family fake TUIs against one server, accepts one message per workspace, closes family, verifies family unavailable while personal remains live, closes personal, and verifies server exit.

- [ ] Run:

```sh
cargo test --release
cargo clippy --release --all-targets
./target/release/brain server status
./target/release/brain receiver status -b brain
```

Expected: all automated tests pass; status commands are read-only; neither command starts a process.

- [ ] Run the lifecycle integration test repeatedly enough to expose race-sensitive failures, using bounded polls and injected clocks rather than adding sleeps.

- [ ] Inspect `git diff --check` and `rg -n 'server (start|kill)|receiver restart|offline queue|brain_root_path\(' src README.md docs` for obsolete behavior.

- [ ] Commit only if authorized; include the required version bump.

## Shared Server and Receiver Exit Criteria

- Different workspace TUIs can coexist; the same workspace still has one TUI.
- One shared process serves all live workspaces without holding portable workspace data globally.
- Final orderly TUI close shuts the process immediately; final crashed lease shuts it after TTL.
- Receiver enablement persists per workspace and all surfaces mutate the same value.
- Accepted messages require enabled receiver plus live target TUI and are never durably queued.
- Workspace ingress routing happens before secrets, users, prompts, and job sockets are selected.
- Manual server start, kill, and restart surfaces no longer exist.
