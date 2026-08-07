const assert = require("node:assert/strict");
const childProcess = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const [pluginPath, scenario] = process.argv.slice(2);

if (!pluginPath || !scenario) {
  throw new Error("usage: plugin_harness.js <plugin-path> <scenario>");
}

globalThis.Bun = {
  spawn(argv, options) {
    const child = childProcess.spawn(argv[0], argv.slice(1), {
      env: options.env,
      stdio: [options.stdin, options.stdout, options.stderr],
    });
    return {
      stdin: child.stdin,
      exited: new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", (code) => resolve(code));
      }),
    };
  },
};

const loadPlugin = async () => {
  const source = fs.readFileSync(pluginPath, "utf8");
  const moduleDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "brain-opencode-module-"));
  const modulePath = path.join(moduleDirectory, "brain.mjs");
  fs.writeFileSync(modulePath, source);
  const module = await import(pathToFileURL(modulePath).href);
  assert.equal(typeof module.BrainPlugin, "function", "BrainPlugin named export must remain loadable");
  return module.BrainPlugin;
};

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

const setupCaptureRoot = () => {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "brain-opencode-plugin-"));
  const root = path.join(temporary, "root with spaces");
  const hookDirectory = path.join(root, ".claude", "brain-hooks");
  const captureFile = path.join(temporary, "capture.jsonl");
  fs.mkdirSync(hookDirectory, { recursive: true });
  for (const name of ["agent_session_start_hook.py", "agent_turn_complete_hook.py"]) {
    fs.writeFileSync(path.join(hookDirectory, name), captureHook, { mode: 0o755 });
  }
  Object.assign(process.env, {
    BRAIN_WORKSPACE_ID: "11111111-1111-4111-8111-111111111111",
    BRAIN_WORKSPACE: "family",
    BRAIN_ROOT: root,
    BRAIN_ACTOR_ID: "member",
    BRAIN_CHANNEL: "sms",
    BRAIN_AGENT_KIND: "opencode",
    BRAIN_INSTANCE_ID: "shell-1",
    BRAIN_PID: "42",
    BRAIN_STATE_DB: path.join(temporary, "state.db"),
    BRAIN_RESPONSE_DIR: path.join(temporary, "responses"),
    BRAIN_RESPONSE_ID: "job-7",
  });
  return { temporary, root, captureFile };
};

const records = (captureFile) => {
  if (!fs.existsSync(captureFile)) return [];
  return fs
    .readFileSync(captureFile, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
};

const completedAssistant = (parts, extra = {}) => ({
  info: {
    role: "assistant",
    time: { created: 10, completed: 11 },
    ...extra,
  },
  parts,
});

const textPart = (text, extra = {}) => ({ type: "text", text, ...extra });

const sdk = ({ sessions = {}, messages = {}, getError, messagesError, malformedMessages } = {}) => {
  const calls = [];
  const logs = [];
  const client = {
    session: {
      async get(args) {
        calls.push({ method: "get", args });
        if (getError) throw new Error(getError);
        return { data: sessions[args.path.id] };
      },
      async messages(args) {
        calls.push({ method: "messages", args });
        if (messagesError) throw new Error(messagesError);
        if (malformedMessages !== undefined) return malformedMessages;
        return { data: messages[args.path.id] ?? [] };
      },
    },
    app: {
      async log(args) {
        logs.push(args);
      },
    },
  };
  return { client, calls, logs };
};

const dispatch = async (plugin, event) => plugin.event({ event });
const created = (info) => ({ type: "session.created", properties: { info } });
const idle = (sessionID) => ({ type: "session.idle", properties: { sessionID } });

const assertExactLookupCalls = (calls, method, sessionIDs, directory) => {
  assert.deepEqual(
    calls.filter((call) => call.method === method).map((call) => call.args),
    sessionIDs.map((sessionID) => ({ path: { id: sessionID }, query: { directory } })),
  );
};

const rootAndChildScenario = async (BrainPlugin) => {
  const { root, captureFile } = setupCaptureRoot();
  const sessions = {
    "root-new": { id: "root-new" },
    "child-new": { id: "child-new", parentID: "root-new" },
    "root-resumed": { id: "root-resumed" },
  };
  const messages = {
    "root-new": [completedAssistant([textPart("new root answer")])],
    "child-new": [completedAssistant([textPart("child answer")])],
    "root-resumed": [completedAssistant([textPart("resumed root answer")])],
  };
  const fake = sdk({ sessions, messages });
  const plugin = await BrainPlugin({ client: fake.client, directory: root });

  await dispatch(plugin, created(sessions["root-new"]));
  await dispatch(plugin, created(sessions["child-new"]));
  await dispatch(plugin, idle("root-new"));
  await dispatch(plugin, idle("child-new"));
  await dispatch(plugin, idle("root-resumed"));

  assertExactLookupCalls(fake.calls, "get", ["root-new", "child-new", "root-resumed"], root);
  assertExactLookupCalls(fake.calls, "messages", ["root-new", "root-resumed"], root);
  assert.deepEqual(
    records(captureFile).map(({ hook, payload }) => ({ hook, payload })),
    [
      {
        hook: "agent_session_start_hook.py",
        payload: { session_id: "root-new", source: "startup" },
      },
      {
        hook: "agent_turn_complete_hook.py",
        payload: { session_id: "root-new", last_assistant_message: "new root answer" },
      },
      {
        hook: "agent_turn_complete_hook.py",
        payload: { session_id: "root-resumed", last_assistant_message: "resumed root answer" },
      },
    ],
  );
  assert.deepEqual(fake.logs, []);
};

const completionScenario = async (BrainPlugin) => {
  const cases = [
    ["no messages", [], undefined],
    ["user only", [{ info: { role: "user", time: { completed: 2 } }, parts: [textPart("question")] }], undefined],
    ["thinking only", [completedAssistant([{ type: "reasoning", text: "private thought" }])], undefined],
    ["tool only", [completedAssistant([{ type: "tool", callID: "tool-1" }])], undefined],
    [
      "newest completed assistant",
      [
        completedAssistant([textPart("older")]),
        { info: { role: "user", time: { completed: 3 } }, parts: [textPart("follow-up")] },
        completedAssistant([textPart("newer")]),
      ],
      "newer",
    ],
    ["multiple text parts", [completedAssistant([textPart("first"), textPart("second")])], "first\n\nsecond"],
    [
      "ignored and synthetic text",
      [completedAssistant([textPart("eligible"), textPart("ignored", { ignored: true }), textPart("synthetic", { synthetic: true })])],
      "eligible",
    ],
    ["assistant error", [completedAssistant([textPart("must not publish")], { error: { name: "ProviderError" } })], undefined],
    ["incomplete assistant", [{ info: { role: "assistant", time: { created: 10 } }, parts: [textPart("partial")] }], undefined],
  ];

  for (const [name, sessionMessages, expected] of cases) {
    const { root, captureFile } = setupCaptureRoot();
    const fake = sdk({ sessions: { root: { id: "root" } }, messages: { root: sessionMessages } });
    const plugin = await BrainPlugin({ client: fake.client, directory: root });
    await dispatch(plugin, idle("root"));
    const stopRecords = records(captureFile).filter(
      (record) => record.hook === "agent_turn_complete_hook.py",
    );
    if (expected === undefined) {
      assert.equal(stopRecords.length, 0, `${name} must not invoke completion`);
    } else {
      assert.equal(stopRecords.length, 1, `${name} must invoke completion once`);
      assert.equal(stopRecords[0].payload.last_assistant_message, expected, name);
    }
  }
};

const errorsScenario = async (BrainPlugin) => {
  const cases = [
    { name: "session lookup", sdkOptions: { getError: "token-secret-get" } },
    {
      name: "message lookup",
      sdkOptions: { sessions: { root: { id: "root" } }, messagesError: "token-secret-messages" },
    },
    {
      name: "malformed messages",
      sdkOptions: { sessions: { root: { id: "root" } }, malformedMessages: { data: { unexpected: true } } },
    },
  ];

  for (const testCase of cases) {
    const { root, captureFile } = setupCaptureRoot();
    const fake = sdk(testCase.sdkOptions);
    const plugin = await BrainPlugin({ client: fake.client, directory: root });
    await dispatch(plugin, idle("root-sensitive-id"));
    assert.equal(records(captureFile).length, 0, `${testCase.name} must not invoke a hook`);
    assert.equal(fake.logs.length, 1, `${testCase.name} must log exactly once`);
    const renderedLog = JSON.stringify(fake.logs[0]);
    assert(!renderedLog.includes("root-sensitive-id"), `${testCase.name} log leaked a session id`);
    assert(!renderedLog.includes("token-secret"), `${testCase.name} log leaked an error detail`);
  }

  const { root, captureFile } = setupCaptureRoot();
  const fake = sdk({
    sessions: { root: { id: "root" } },
    messages: { root: [completedAssistant([textPart("secret assistant text")])] },
  });
  fs.writeFileSync(
    path.join(root, ".claude", "brain-hooks", "agent_turn_complete_hook.py"),
    "#!/usr/bin/env python3\nraise SystemExit(17)\n",
    { mode: 0o755 },
  );
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, idle("root"));
  assert.equal(records(captureFile).length, 0, "a failing hook must not emit completion capture");
  assert.equal(fake.logs.length, 1, "a failing hook must be logged");
  assert(!JSON.stringify(fake.logs[0]).includes("secret assistant text"), "hook failure log leaked assistant text");
};

const subprocessSafetyScenario = async (BrainPlugin) => {
  const { temporary, root, captureFile } = setupCaptureRoot();
  const sentinel = path.join(temporary, "injected-command-ran");
  const sessionID = `root'; touch '${sentinel}'; #`;
  const assistantText = `literal $(touch '${sentinel}') and \`touch '${sentinel}'\``;
  process.env.SECRET_DO_NOT_FORWARD = "top-secret-value";
  process.env.BRAIN_API_TOKEN = "brain-secret-value";
  const fake = sdk({
    sessions: { [sessionID]: { id: sessionID } },
    messages: { [sessionID]: [completedAssistant([textPart(assistantText)])] },
  });
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, idle(sessionID));

  assert.equal(fs.existsSync(sentinel), false, "session content must never execute through a shell");
  const [record] = records(captureFile);
  assert.equal(record.payload.session_id, sessionID);
  assert.equal(record.payload.last_assistant_message, assistantText);
  assert.equal(record.secret_forwarded, false, "ambient secrets must not reach hook subprocesses");
  assert.equal(record.env.BRAIN_ROOT, root);
  assert.equal(record.env.BRAIN_AGENT_KIND, "opencode");
  assert.equal(record.env.BRAIN_RESPONSE_DIR, path.join(temporary, "responses"));
  assert.equal(record.env.BRAIN_API_TOKEN, undefined);
  assert.equal(typeof record.env.PATH, "string");
};

const repeatedIdleScenario = async (BrainPlugin) => {
  const directory = process.env.BRAIN_ROOT;
  assert(directory, "BRAIN_ROOT is required for the real bridge scenario");
  const fake = sdk({
    sessions: { "root-real": { id: "root-real" } },
    messages: { "root-real": [completedAssistant([textPart("Completed once")])] },
  });
  const plugin = await BrainPlugin({ client: fake.client, directory });
  await dispatch(plugin, created({ id: "root-real" }));
  await dispatch(plugin, idle("root-real"));
  await dispatch(plugin, idle("root-real"));
  assertExactLookupCalls(fake.calls, "get", ["root-real", "root-real"], directory);
  assert.equal(fake.logs.length, 0);
};

const newSessionScenario = async (BrainPlugin) => {
  const directory = process.env.BRAIN_ROOT;
  assert(directory, "BRAIN_ROOT is required for the new-session bridge scenario");
  const fake = sdk();
  const plugin = await BrainPlugin({ client: fake.client, directory });

  await dispatch(plugin, created({ id: "root-before-new" }));
  await dispatch(plugin, created({ id: "root-after-new" }));

  assert.deepEqual(fake.calls, []);
  assert.deepEqual(fake.logs, []);
};

(async () => {
  const BrainPlugin = await loadPlugin();
  const scenarios = {
    roots: rootAndChildScenario,
    completion: completionScenario,
    errors: errorsScenario,
    safety: subprocessSafetyScenario,
    repeated_idle: repeatedIdleScenario,
    new_session: newSessionScenario,
  };
  const run = scenarios[scenario];
  if (!run) throw new Error(`unknown scenario: ${scenario}`);
  await run(BrainPlugin);
  process.stdout.write(JSON.stringify({ ok: true, scenario }));
})().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});
