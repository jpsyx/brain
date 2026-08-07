// Brain lifecycle bridge for OpenCode.
//
// Frontend event and SDK details stay here. Brain's existing Python hooks own
// session rotation, attribution, deduplication, and response publication.

const RUNTIME_ENVIRONMENT = [
  "PATH",
  "HOME",
  "TMPDIR",
  "TMP",
  "TEMP",
  "LANG",
  "LC_ALL",
  "LC_CTYPE",
];

const BRAIN_ENVIRONMENT = [
  "BRAIN_WORKSPACE_ID",
  "BRAIN_WORKSPACE",
  "BRAIN_ROOT",
  "BRAIN_ACTOR_ID",
  "BRAIN_CHANNEL",
  "BRAIN_AGENT_KIND",
  "BRAIN_INSTANCE_ID",
  "BRAIN_PID",
  "BRAIN_STATE_DB",
  "BRAIN_RESPONSE_DIR",
  "BRAIN_RESPONSE_ID",
];

const sessionIdFrom = (event) => {
  const properties = event?.properties ?? {};
  return properties.sessionID ?? properties.session_id ?? properties.info?.id;
};

const isRootSession = (info) => !info?.parentID && !info?.parentId;

const hookEnvironment = () =>
  Object.fromEntries(
    Object.entries(process.env).filter(
      ([name, value]) =>
        value !== undefined &&
        (BRAIN_ENVIRONMENT.includes(name) || RUNTIME_ENVIRONMENT.includes(name)),
    ),
  );

const runHook = async (hook, payload) => {
  const child = Bun.spawn(["python3", hook], {
    stdin: "pipe",
    stdout: "ignore",
    stderr: "ignore",
    env: hookEnvironment(),
  });
  child.stdin.write(JSON.stringify(payload));
  child.stdin.end();
  const exitCode = await child.exited;
  if (exitCode !== 0) throw new Error("Brain lifecycle hook failed");
};

const hookPath = (name) => {
  const root = process.env.BRAIN_ROOT;
  if (!root) return undefined;
  return `${root}/.claude/brain-hooks/${name}`;
};

const completedAssistantText = (messages) => {
  const message = [...messages].reverse().find((candidate) => {
    const info = candidate?.info;
    return (
      info?.role === "assistant" &&
      info.error == null &&
      info.time?.completed !== undefined &&
      info.time?.completed !== null
    );
  });
  if (!message) return undefined;
  const text = (Array.isArray(message.parts) ? message.parts : [])
    .filter(
      (part) =>
        part?.type === "text" &&
        typeof part.text === "string" &&
        !part.ignored &&
        !part.synthetic,
    )
    .map((part) => part.text)
    .filter((part) => part.length > 0)
    .join("\n\n");
  return text.trim() ? text : undefined;
};

const responseData = (response) =>
  response && Object.prototype.hasOwnProperty.call(response, "data") ? response.data : response;

const logFailure = async (client, operation) => {
  try {
    await client.app.log({
      body: {
        service: "brain",
        level: "error",
        message: "OpenCode Brain lifecycle integration failed",
        extra: { operation },
      },
    });
  } catch {
    // OpenCode logging failures cannot safely be recovered inside an event hook.
  }
};

const invokeHook = async (client, operation, hook, payload) => {
  if (!hook) return;
  try {
    await runHook(hook, payload);
  } catch {
    await logFailure(client, operation);
  }
};

const handleIdle = async (client, directory, sessionID) => {
  let info;
  try {
    info = responseData(
      await client.session.get({ path: { id: sessionID }, query: { directory } }),
    );
  } catch {
    await logFailure(client, "session_lookup");
    return;
  }
  if (!info || typeof info !== "object" || typeof info.id !== "string") {
    await logFailure(client, "session_lookup_response");
    return;
  }
  if (!isRootSession(info)) return;

  let messages;
  try {
    messages = responseData(
      await client.session.messages({ path: { id: sessionID }, query: { directory } }),
    );
  } catch {
    await logFailure(client, "message_lookup");
    return;
  }
  if (!Array.isArray(messages)) {
    await logFailure(client, "message_lookup_response");
    return;
  }
  const message = completedAssistantText(messages);
  if (!message) return;
  await invokeHook(client, "turn_complete_bridge", hookPath("agent_turn_complete_hook.py"), {
    session_id: sessionID,
    last_assistant_message: message,
  });
};

export const BrainPlugin = async ({ client, directory }) => ({
  event: async ({ event }) => {
    const sessionID = sessionIdFrom(event);
    if (!sessionID) return;

    if (event.type === "session.created") {
      if (!isRootSession(event.properties?.info)) return;
      await invokeHook(
        client,
        "session_start_bridge",
        hookPath("agent_session_start_hook.py"),
        { session_id: sessionID, source: "startup" },
      );
      return;
    }

    if (event.type === "session.idle") await handleIdle(client, directory, sessionID);
  },
});
