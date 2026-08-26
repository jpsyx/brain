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
  "BRAIN_RECEIVER_JOB_TOKEN",
  "BRAIN_RECEIVER_OBSERVATION_PATH",
];

const MAX_CORRELATION_ENTRIES = 32;

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
  return `${root}/.brain/hooks/${name}`;
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
  if (!hook) return false;
  try {
    await runHook(hook, payload);
    return true;
  } catch {
    await logFailure(client, operation);
    return false;
  }
};

const boundedSet = (values, key, value) => {
  values.delete(key);
  values.set(key, value);
  while (values.size > MAX_CORRELATION_ENTRIES) {
    values.delete(values.keys().next().value);
  }
};

const exactReceiverMarker = (value) => {
  const token = process.env.BRAIN_RECEIVER_JOB_TOKEN;
  if (typeof value !== "string" || typeof token !== "string" || !token) return false;
  const withoutTerminalNewline = value.replace(/\r?\n$/, "");
  const lines = withoutTerminalNewline.split(/\r?\n/);
  return lines.at(-1) === `<!-- brain:receiver-job-token=${token} -->`;
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
  await invokeHook(client, "session_stop_bridge", hookPath("agent_session_stop_hook.py"), {
    session_id: sessionID,
    last_assistant_message: message,
  });
};

export const BrainPlugin = async ({ client, directory }) => {
  const rootSessions = new Map();
  const userMessages = new Map();
  const acceptedSessions = new Map();

  return {
    event: async ({ event }) => {
      const sessionID = sessionIdFrom(event);

      if (event.type === "session.created") {
        if (!sessionID) return;
        const root = isRootSession(event.properties?.info);
        boundedSet(rootSessions, sessionID, root);
        if (!root) return;
        await invokeHook(
          client,
          "session_start_bridge",
          hookPath("agent_session_start_hook.py"),
          { session_id: sessionID, source: "startup" },
        );
        return;
      }

      if (event.type === "session.updated") {
        const info = event.properties?.info;
        if (!sessionID || typeof info?.id !== "string" || info.id !== sessionID) return;
        boundedSet(rootSessions, sessionID, isRootSession(info));
        return;
      }

      if (event.type === "message.updated") {
        const info = event.properties?.info;
        if (
          info?.role === "user" &&
          typeof info.id === "string" &&
          typeof info.sessionID === "string" &&
          rootSessions.get(info.sessionID) === true
        ) {
          boundedSet(userMessages, info.id, info.sessionID);
        }
        return;
      }

      if (event.type === "message.part.updated") {
        const part = event.properties?.part;
        if (
          part?.type !== "text" ||
          typeof part.messageID !== "string" ||
          typeof part.sessionID !== "string" ||
          userMessages.get(part.messageID) !== part.sessionID ||
          !exactReceiverMarker(part.text)
        ) {
          return;
        }
        const accepted = await invokeHook(
          client,
          "receiver_acceptance_bridge",
          hookPath("receiver_observation_bridge.py"),
          {
            hook_event_name: "UserPromptSubmit",
            session_id: part.sessionID,
            prompt: part.text,
          },
        );
        if (accepted) boundedSet(acceptedSessions, part.sessionID, true);
        return;
      }

      if (event.type === "session.idle" && sessionID) {
        await handleIdle(client, directory, sessionID);
      }
    },
    "tool.execute.after": async (input) => {
      if (!acceptedSessions.has(input?.sessionID)) return;
      await invokeHook(
        client,
        "receiver_progress_bridge",
        hookPath("receiver_observation_bridge.py"),
        {
          hook_event_name: "PostToolUse",
          session_id: input.sessionID,
          turn_id: typeof input.messageID === "string" ? input.messageID : null,
        },
      );
    },
  };
};
