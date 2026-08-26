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
  const hookDirectory = path.join(root, ".brain", "hooks");
  const captureFile = path.join(temporary, "capture.jsonl");
  fs.mkdirSync(hookDirectory, { recursive: true });
  for (const name of ["agent_session_start_hook.py", "agent_session_stop_hook.py"]) {
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
const updated = (info) => ({ type: "session.updated", properties: { info } });
const idle = (sessionID) => ({ type: "session.idle", properties: { sessionID } });
const messageUpdated = (info) => ({ type: "message.updated", properties: { info } });
const partUpdated = (part) => ({ type: "message.part.updated", properties: { part } });

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
        hook: "agent_session_stop_hook.py",
        payload: { session_id: "root-new", last_assistant_message: "new root answer" },
      },
      {
        hook: "agent_session_stop_hook.py",
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
      (record) => record.hook === "agent_session_stop_hook.py",
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
    path.join(root, ".brain", "hooks", "agent_session_stop_hook.py"),
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

const observationScenario = async (BrainPlugin) => {
  const { temporary, root } = setupCaptureRoot();
  const token = "11111111-1111-4111-8111-111111111111";
  const observationPath = path.join(temporary, "observations", "receiver.json");
  fs.copyFileSync(
    path.join(path.dirname(pluginPath), "receiver_observation_bridge.py"),
    path.join(root, ".brain", "hooks", "receiver_observation_bridge.py"),
  );
  Object.assign(process.env, {
    BRAIN_RECEIVER_JOB_TOKEN: token,
    BRAIN_RECEIVER_OBSERVATION_PATH: observationPath,
    BRAIN_INSTANCE_ID: "22222222-2222-4222-8222-222222222222",
  });
  const fake = sdk();
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, created({ id: "root-observed" }));
  await dispatch(plugin, created({ id: "child-observed", parentID: "root-observed" }));

  const marker = `<!-- brain:receiver-job-token=${token} -->`;
  await dispatch(
    plugin,
    messageUpdated({
      id: "child-user",
      sessionID: "child-observed",
      role: "user",
      time: { created: 0 },
    }),
  );
  await dispatch(
    plugin,
    partUpdated({
      id: "child-part",
      sessionID: "child-observed",
      messageID: "child-user",
      type: "text",
      text: marker,
    }),
  );
  assert.equal(fs.existsSync(observationPath), false, "child correlation must not accept");

  for (let index = 0; index < 40; index += 1) {
    await dispatch(
      plugin,
      messageUpdated({
        id: `user-${index}`,
        sessionID: "root-observed",
        role: "user",
        time: { created: index },
      }),
    );
  }
  await dispatch(
    plugin,
    partUpdated({
      id: "part-evicted",
      sessionID: "root-observed",
      messageID: "user-0",
      type: "text",
      text: marker,
    }),
  );
  assert.equal(fs.existsSync(observationPath), false, "evicted correlation must not accept");
  await dispatch(
    plugin,
    partUpdated({
      id: "part-current",
      sessionID: "root-observed",
      messageID: "user-39",
      type: "text",
      text: `synthetic\n${marker}`,
    }),
  );

  const accepted = JSON.parse(fs.readFileSync(observationPath, "utf8"));
  assert.equal(accepted.phase, "accepted");
  assert.equal(accepted.revision, 1);
  assert.equal(accepted.session_id, "root-observed");
  assert.equal(accepted.job_token, token);
  assert.deepEqual(fake.calls, [], "acceptance must not fetch message history");

  await plugin["tool.execute.after"](
    { sessionID: "other-session", messageID: "other-turn", tool: "synthetic-tool" },
    { output: "synthetic-output" },
  );
  assert.equal(JSON.parse(fs.readFileSync(observationPath, "utf8")).revision, 1);
  await plugin["tool.execute.after"](
    { sessionID: "root-observed", messageID: "turn-1", tool: "synthetic-tool" },
    { output: "synthetic-output" },
  );
  const progressing = JSON.parse(fs.readFileSync(observationPath, "utf8"));
  assert.equal(progressing.phase, "progressing");
  assert.equal(progressing.revision, 2);
  assert.equal(progressing.turn_id, "turn-1");
  assert.deepEqual(fake.calls, [], "progress must not fetch message history");
  const serialized = JSON.stringify(progressing);
  for (const forbidden of ["synthetic", "tool", "output", "sender", "recipient", "cwd"]) {
    assert.equal(serialized.includes(forbidden), false, `snapshot leaked ${forbidden}`);
  }
};

const resumedObservationScenario = async (BrainPlugin) => {
  const { temporary, root } = setupCaptureRoot();
  const token = "11111111-1111-4111-8111-111111111111";
  const observationPath = path.join(temporary, "observations", "receiver.json");
  fs.copyFileSync(
    path.join(path.dirname(pluginPath), "receiver_observation_bridge.py"),
    path.join(root, ".brain", "hooks", "receiver_observation_bridge.py"),
  );
  Object.assign(process.env, {
    BRAIN_RECEIVER_JOB_TOKEN: token,
    BRAIN_RECEIVER_OBSERVATION_PATH: observationPath,
    BRAIN_INSTANCE_ID: "22222222-2222-4222-8222-222222222222",
  });
  const fake = sdk();
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, updated({ id: "root-resumed" }));
  await dispatch(plugin, updated({ id: "child-resumed", parentID: "root-resumed" }));

  const marker = `<!-- brain:receiver-job-token=${token} -->`;
  await dispatch(
    plugin,
    messageUpdated({ id: "child-user", sessionID: "child-resumed", role: "user" }),
  );
  await dispatch(
    plugin,
    partUpdated({
      id: "child-part",
      sessionID: "child-resumed",
      messageID: "child-user",
      type: "text",
      text: marker,
    }),
  );
  assert.equal(fs.existsSync(observationPath), false, "resumed child must not accept");

  await dispatch(
    plugin,
    messageUpdated({ id: "root-user", sessionID: "root-resumed", role: "user" }),
  );
  await dispatch(
    plugin,
    partUpdated({
      id: "root-part",
      sessionID: "root-resumed",
      messageID: "root-user",
      type: "text",
      text: marker,
    }),
  );
  const accepted = JSON.parse(fs.readFileSync(observationPath, "utf8"));
  assert.equal(accepted.phase, "accepted");
  assert.equal(accepted.session_id, "root-resumed");

  await plugin["tool.execute.after"]({ sessionID: "root-resumed", messageID: "turn-resumed" });
  const progressing = JSON.parse(fs.readFileSync(observationPath, "utf8"));
  assert.equal(progressing.phase, "progressing");
  assert.equal(progressing.turn_id, "turn-resumed");
  assert.deepEqual(fake.calls, [], "resumed evidence must not fetch message history");
};

const externalObservationScenario = async (BrainPlugin) => {
  const root = process.env.BRAIN_ROOT;
  const token = process.env.BRAIN_RECEIVER_JOB_TOKEN;
  const observationPath = process.env.BRAIN_RECEIVER_OBSERVATION_PATH;
  const sessionID = process.env.TEST_RECEIVER_SESSION_ID;
  assert(root, "BRAIN_ROOT is required");
  assert(token, "BRAIN_RECEIVER_JOB_TOKEN is required");
  assert(observationPath, "BRAIN_RECEIVER_OBSERVATION_PATH is required");
  assert(sessionID, "TEST_RECEIVER_SESSION_ID is required");
  const hookDirectory = path.join(root, ".brain", "hooks");
  fs.mkdirSync(hookDirectory, { recursive: true });
  fs.copyFileSync(
    path.join(path.dirname(pluginPath), "receiver_observation_bridge.py"),
    path.join(hookDirectory, "receiver_observation_bridge.py"),
  );
  const fake = sdk();
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, updated({ id: sessionID }));

  const beforeReorderedProgress = fs.existsSync(observationPath)
    ? fs.readFileSync(observationPath)
    : undefined;
  await plugin["tool.execute.after"]({ sessionID, messageID: "turn-before-acceptance" });
  if (beforeReorderedProgress === undefined) {
    assert.equal(fs.existsSync(observationPath), false, "reordered progress must not accept");
  } else {
    assert.deepEqual(
      fs.readFileSync(observationPath),
      beforeReorderedProgress,
      "reordered progress must not mutate prior evidence",
    );
  }

  const messageID = "user-current";
  const marker = `<!-- brain:receiver-job-token=${token} -->`;
  await dispatch(plugin, messageUpdated({ id: messageID, sessionID, role: "user" }));
  const part = {
    id: "part-current",
    sessionID,
    messageID,
    type: "text",
    text: `synthetic\n${marker}`,
  };
  await dispatch(plugin, partUpdated(part));
  await dispatch(plugin, partUpdated(part));
  await plugin["tool.execute.after"]({ sessionID, messageID: "turn-current" });
  await plugin["tool.execute.after"]({ sessionID, messageID: "turn-duplicate" });

  const snapshot = JSON.parse(fs.readFileSync(observationPath, "utf8"));
  assert.equal(snapshot.phase, "progressing");
  assert.equal(snapshot.revision, 2);
  assert.equal(snapshot.session_id, sessionID);
  assert.deepEqual(fake.calls, [], "incremental observation must not fetch history");
};

const externalObservationStageScenario = async (BrainPlugin) => {
  const root = process.env.BRAIN_ROOT;
  const token = process.env.BRAIN_RECEIVER_JOB_TOKEN;
  const observationPath = process.env.BRAIN_RECEIVER_OBSERVATION_PATH;
  const sessionID = process.env.TEST_RECEIVER_SESSION_ID;
  const stage = process.env.TEST_RECEIVER_STAGE;
  assert(root, "BRAIN_ROOT is required");
  assert(token, "BRAIN_RECEIVER_JOB_TOKEN is required");
  assert(observationPath, "BRAIN_RECEIVER_OBSERVATION_PATH is required");
  assert(sessionID, "TEST_RECEIVER_SESSION_ID is required");
  assert(stage, "TEST_RECEIVER_STAGE is required");
  const hookDirectory = path.join(root, ".brain", "hooks");
  fs.mkdirSync(hookDirectory, { recursive: true });
  for (const name of ["receiver_observation_bridge.py", "agent_session_stop_hook.py"]) {
    fs.copyFileSync(path.join(path.dirname(pluginPath), name), path.join(hookDirectory, name));
  }
  const fake = sdk({
    sessions: { [sessionID]: { id: sessionID } },
    messages: { [sessionID]: [completedAssistant([textPart("matrix completion")])] },
  });
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, updated({ id: sessionID }));

  if (stage === "reordered_progress") {
    await plugin["tool.execute.after"]({ sessionID, messageID: "turn-before-acceptance" });
  } else if (stage === "accepted" || stage === "progressing") {
    const messageID = "matrix-user";
    const part = {
      id: "matrix-part",
      sessionID,
      messageID,
      type: "text",
      text: `matrix\n<!-- brain:receiver-job-token=${token} -->`,
    };
    await dispatch(plugin, messageUpdated({ id: messageID, sessionID, role: "user" }));
    await dispatch(plugin, partUpdated(part));
    if (stage === "progressing") {
      await plugin["tool.execute.after"]({ sessionID, messageID: "matrix-turn" });
    }
  } else if (stage === "completed") {
    await dispatch(plugin, idle(sessionID));
  } else {
    throw new Error(`unknown receiver stage: ${stage}`);
  }
  assert.deepEqual(fake.logs, []);
};

const externalObservationPrivacyScenario = async (BrainPlugin) => {
  const root = process.env.BRAIN_ROOT;
  const token = process.env.BRAIN_RECEIVER_JOB_TOKEN;
  const sessionID = process.env.TEST_RECEIVER_SESSION_ID;
  const promptCanary = process.env.TEST_PROMPT_CANARY;
  const responseCanary = process.env.TEST_RESPONSE_CANARY;
  assert(root && token && sessionID && promptCanary && responseCanary);
  const hookDirectory = path.join(root, ".brain", "hooks");
  fs.mkdirSync(hookDirectory, { recursive: true });
  for (const name of ["receiver_observation_bridge.py", "agent_session_stop_hook.py"]) {
    fs.copyFileSync(path.join(path.dirname(pluginPath), name), path.join(hookDirectory, name));
  }
  const fake = sdk({
    sessions: { [sessionID]: { id: sessionID } },
    messages: { [sessionID]: [completedAssistant([textPart(responseCanary)])] },
  });
  const plugin = await BrainPlugin({ client: fake.client, directory: root });
  await dispatch(plugin, updated({ id: sessionID }));
  const messageID = "privacy-user";
  await dispatch(plugin, messageUpdated({ id: messageID, sessionID, role: "user" }));
  await dispatch(
    plugin,
    partUpdated({
      id: "privacy-part",
      sessionID,
      messageID,
      type: "text",
      text: `${promptCanary}\n<!-- brain:receiver-job-token=${token} -->`,
    }),
  );
  await plugin["tool.execute.after"]({
    sessionID,
    messageID: "privacy-turn",
    body: process.env.TEST_BODY_CANARY,
    recipient: process.env.TEST_RECIPIENT_CANARY,
    credential: process.env.TEST_CREDENTIAL_CANARY,
  });
  await dispatch(plugin, idle(sessionID));
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
    observations: observationScenario,
    resumed_observations: resumedObservationScenario,
    external_observation: externalObservationScenario,
    external_observation_stage: externalObservationStageScenario,
    external_observation_privacy: externalObservationPrivacyScenario,
  };
  const run = scenarios[scenario];
  if (!run) throw new Error(`unknown scenario: ${scenario}`);
  await run(BrainPlugin);
  process.stdout.write(JSON.stringify({ ok: true, scenario }));
})().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});
