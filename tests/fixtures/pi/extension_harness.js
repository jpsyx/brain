const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const [extensionPath, scenario] = process.argv.slice(2);

if (!extensionPath || !scenario) {
  throw new Error("usage: extension_harness.js <extension-path> <scenario>");
}

const JOB_TOKEN = "11111111-1111-4111-8111-111111111111";
const MARKER = `<!-- brain:receiver-job-token=${JOB_TOKEN} -->`;

const captureHook = `#!/usr/bin/env python3
import json
import os
import pathlib
import sys

hook = pathlib.Path(sys.argv[0]).name
payload = json.load(sys.stdin)
record = {
    "hook": hook,
    "payload": payload,
    "env": {
        key: value
        for key, value in os.environ.items()
        if key.startswith("BRAIN_") or key in ("PATH", "HOME", "TMPDIR", "LANG", "LC_ALL")
    },
    "secret_forwarded": "SECRET_DO_NOT_FORWARD" in os.environ,
}
capture_file = pathlib.Path(os.environ["BRAIN_RESPONSE_DIR"]).parent / "capture.jsonl"
with open(capture_file, "a", encoding="utf-8") as output:
    output.write(json.dumps(record) + "\\n")
`;

const HOOKS = [
  "agent_session_start_hook.py",
  "agent_session_stop_hook.py",
  "receiver_observation_bridge.py",
];

const setupCaptureRoot = () => {
  const temporary = fs.realpathSync(
    fs.mkdtempSync(path.join(os.tmpdir(), "brain-pi-extension-")),
  );
  const root = path.join(temporary, "root with spaces");
  const hookDirectory = path.join(root, ".brain", "hooks");
  const captureFile = path.join(temporary, "capture.jsonl");
  fs.mkdirSync(hookDirectory, { recursive: true });
  for (const name of HOOKS) {
    fs.writeFileSync(path.join(hookDirectory, name), captureHook, { mode: 0o755 });
  }
  Object.assign(process.env, {
    BRAIN_WORKSPACE_ID: "11111111-1111-4111-8111-111111111111",
    BRAIN_WORKSPACE: "family",
    BRAIN_ROOT: root,
    BRAIN_ACTOR_ID: "member",
    BRAIN_CHANNEL: "sms",
    BRAIN_AGENT_KIND: "pi",
    BRAIN_INSTANCE_ID: "shell-1",
    BRAIN_PID: "42",
    BRAIN_STATE_DB: path.join(temporary, "state.db"),
    BRAIN_RESPONSE_DIR: path.join(temporary, "responses"),
    BRAIN_RESPONSE_ID: "job-7",
    BRAIN_RECEIVER_JOB_TOKEN: JOB_TOKEN,
    SECRET_DO_NOT_FORWARD: "never",
  });
  return { temporary, captureFile };
};

const records = (captureFile) => {
  if (!fs.existsSync(captureFile)) return [];
  return fs
    .readFileSync(captureFile, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
};

/// pi loads a `.ts` extension through jiti; the bridge is plain JavaScript, so
/// the harness imports the same source as an ES module.
const loadExtension = async () => {
  const source = fs.readFileSync(extensionPath, "utf8");
  const moduleDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "brain-pi-module-"));
  const modulePath = path.join(moduleDirectory, "brain.mjs");
  fs.writeFileSync(modulePath, source);
  const module = await import(pathToFileURL(modulePath).href);
  assert.equal(typeof module.default, "function", "pi extensions export a default factory");
  return module.default;
};

/// The slice of pi's ExtensionAPI and ExtensionContext the bridge uses.
const createPi = () => {
  const handlers = new Map();
  const notifications = [];
  return {
    api: {
      on(event, handler) {
        handlers.set(event, handler);
      },
    },
    notifications,
    registered: () => [...handlers.keys()],
    async emit(event, payload, sessionId) {
      const handler = handlers.get(event);
      assert.ok(handler, `the bridge must handle ${event}`);
      const ctx = {
        sessionManager: { getSessionId: () => sessionId },
        ui: {
          notify(message, level) {
            notifications.push({ message, level });
          },
        },
      };
      return handler(payload, ctx);
    },
  };
};

const assistantMessage = (text, extra = {}) => ({
  message: {
    role: "assistant",
    stopReason: "stop",
    content: [{ type: "text", text }],
    ...extra,
  },
});

const scenarios = {
  async session_start() {
    const { captureFile } = setupCaptureRoot();
    const pi = createPi();
    (await loadExtension())(pi.api);

    await pi.emit("session_start", { reason: "startup" }, "session-1");
    await pi.emit("session_start", { reason: "fork" }, "session-2");

    const captured = records(captureFile);
    assert.deepEqual(
      captured.map((record) => [record.hook, record.payload]),
      [
        ["agent_session_start_hook.py", { session_id: "session-1", source: "startup" }],
        // A fork is passed through as-is; the bridge, not the extension, is the
        // one place that decides a fork continues no lineage.
        ["agent_session_start_hook.py", { session_id: "session-2", source: "fork" }],
      ],
    );
    // An ephemeral pi session has no id, and there is nothing to record.
    await pi.emit("session_start", { reason: "new" }, undefined);
    assert.equal(records(captureFile).length, 2);
  },

  async completion() {
    const { captureFile } = setupCaptureRoot();
    const pi = createPi();
    (await loadExtension())(pi.api);
    await pi.emit("session_start", { reason: "startup" }, "session-1");

    await pi.emit("message_end", assistantMessage("first answer"), "session-1");
    await pi.emit("message_end", assistantMessage("final answer"), "session-1");
    // Neither a user message nor an errored or aborted turn is an answer.
    await pi.emit("message_end", { message: { role: "user", content: "hi" } }, "session-1");
    await pi.emit("agent_settled", {}, "session-1");
    // Settling again publishes nothing: the answer was already delivered.
    await pi.emit("agent_settled", {}, "session-1");

    const stops = records(captureFile).filter(
      (record) => record.hook === "agent_session_stop_hook.py",
    );
    assert.deepEqual(stops.map((record) => record.payload), [
      { session_id: "session-1", last_assistant_message: "final answer" },
    ]);
  },

  async errored_turn() {
    const { captureFile } = setupCaptureRoot();
    const pi = createPi();
    (await loadExtension())(pi.api);
    await pi.emit("session_start", { reason: "startup" }, "session-1");

    await pi.emit(
      "message_end",
      assistantMessage("half an answer", { stopReason: "error", errorMessage: "boom" }),
      "session-1",
    );
    await pi.emit(
      "message_end",
      assistantMessage("abandoned", { stopReason: "aborted" }),
      "session-1",
    );
    await pi.emit("agent_settled", {}, "session-1");

    assert.deepEqual(
      records(captureFile).filter((record) => record.hook === "agent_session_stop_hook.py"),
      [],
    );
  },

  async observations() {
    const { captureFile } = setupCaptureRoot();
    const pi = createPi();
    (await loadExtension())(pi.api);
    await pi.emit("session_start", { reason: "startup" }, "session-1");

    // An ordinary prompt carries no receiver authority, so its tool events
    // cannot publish progress.
    await pi.emit("input", { text: "ordinary prompt", source: "interactive" }, "session-1");
    await pi.emit("tool_execution_end", { toolCallId: "call-0" }, "session-1");
    assert.deepEqual(
      records(captureFile).filter((record) => record.hook === "receiver_observation_bridge.py"),
      [],
    );

    await pi.emit(
      "input",
      { text: `authenticated message\n${MARKER}`, source: "rpc" },
      "session-1",
    );
    await pi.emit("tool_execution_end", { toolCallId: "call-1" }, "session-1");
    await pi.emit("tool_execution_end", { toolCallId: "call-2" }, "session-1");

    const observations = records(captureFile)
      .filter((record) => record.hook === "receiver_observation_bridge.py")
      .map((record) => record.payload);
    assert.equal(observations.length, 3);
    const [accepted, first, second] = observations;
    assert.equal(accepted.hook_event_name, "UserPromptSubmit");
    assert.equal(accepted.session_id, "session-1");
    assert.match(
      accepted.turn_id,
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
    for (const [progress, toolCallId] of [
      [first, "call-1"],
      [second, "call-2"],
    ]) {
      assert.equal(progress.hook_event_name, "PostToolUse");
      assert.equal(progress.session_id, "session-1");
      // Progress stays inside the one turn the bridge authorized.
      assert.equal(progress.turn_id, accepted.turn_id);
      assert.equal(progress.tool_use_id, toolCallId);
    }

    // A later ordinary prompt revokes that authority.
    await pi.emit("input", { text: "later prompt", source: "interactive" }, "session-1");
    await pi.emit("tool_execution_end", { toolCallId: "call-3" }, "session-1");
    assert.equal(
      records(captureFile).filter((record) => record.hook === "receiver_observation_bridge.py")
        .length,
      3,
    );
  },

  async safety() {
    const { captureFile } = setupCaptureRoot();
    const pi = createPi();
    (await loadExtension())(pi.api);

    await pi.emit("session_start", { reason: "startup" }, "session-1");

    const [record] = records(captureFile);
    assert.equal(record.secret_forwarded, false, "unrelated environment must not reach a hook");
    assert.deepEqual(
      Object.keys(record.env).filter((name) => name.startsWith("BRAIN_")).sort(),
      [
        "BRAIN_ACTOR_ID",
        "BRAIN_AGENT_KIND",
        "BRAIN_CHANNEL",
        "BRAIN_INSTANCE_ID",
        "BRAIN_PID",
        "BRAIN_RECEIVER_JOB_TOKEN",
        "BRAIN_RESPONSE_DIR",
        "BRAIN_RESPONSE_ID",
        "BRAIN_ROOT",
        "BRAIN_STATE_DB",
        "BRAIN_WORKSPACE",
        "BRAIN_WORKSPACE_ID",
      ],
    );
  },

  async no_root() {
    setupCaptureRoot();
    delete process.env.BRAIN_ROOT;
    const pi = createPi();
    (await loadExtension())(pi.api);

    // Without a selected workspace there is no bridge to call, and a direct pi
    // session must not fail because Brain's extension was loaded.
    await pi.emit("session_start", { reason: "startup" }, "session-1");
    await pi.emit("message_end", assistantMessage("answer"), "session-1");
    await pi.emit("agent_settled", {}, "session-1");
  },

  /// The extension, the real Python bridges, and the real state database. The
  /// environment comes from the Rust test, so nothing here rewrites it.
  async real_bridges() {
    const pi = createPi();
    (await loadExtension())(pi.api);

    await pi.emit("session_start", { reason: "startup" }, "rotated-pi-session");
    await pi.emit("message_end", assistantMessage("settled answer"), "rotated-pi-session");
    await pi.emit("agent_settled", {}, "rotated-pi-session");
    // A second settle has nothing left to publish, so the one artifact stands.
    await pi.emit("agent_settled", {}, "rotated-pi-session");
  },

  async registration() {
    setupCaptureRoot();
    const pi = createPi();
    (await loadExtension())(pi.api);

    assert.deepEqual(pi.registered().sort(), [
      "agent_settled",
      "input",
      "message_end",
      "session_start",
      "tool_execution_end",
    ]);
  },
};

const run = async () => {
  const selected = scenarios[scenario];
  if (!selected) {
    throw new Error(`unknown scenario ${scenario}`);
  }
  await selected();
};

run().catch((error) => {
  console.error(error);
  process.exit(1);
});
