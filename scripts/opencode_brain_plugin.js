// Brain lifecycle bridge for OpenCode.
//
// The plugin keeps frontend-specific event handling here. Database mutation,
// attribution, deduplication, and response publication remain in Brain's
// existing Python lifecycle hooks.

const sessionIdFrom = (event) => {
  const properties = event?.properties ?? {};
  return properties.sessionID ?? properties.session_id ?? properties.info?.id;
};

const isRootSession = (event) => {
  const info = event?.properties?.info;
  return !info?.parentID && !info?.parentId;
};

const runHook = async (hook, payload) => {
  const child = Bun.spawn(["python3", hook], {
    stdin: "pipe",
    stdout: "ignore",
    stderr: "ignore",
    env: process.env,
  });
  child.stdin.write(JSON.stringify(payload));
  child.stdin.end();
  await child.exited;
};

const hookPath = (name) => {
  const root = process.env.BRAIN_ROOT;
  if (!root) return undefined;
  return `${root}/.claude/brain-hooks/${name}`;
};

const latestAssistantText = (messages) => {
  for (const message of [...messages].reverse()) {
    if (message?.info?.role !== "assistant") continue;
    const text = (message.parts ?? [])
      .filter((part) => part?.type === "text" && typeof part.text === "string")
      .map((part) => part.text)
      .filter(Boolean)
      .join("\n\n");
    if (text.trim()) return text;
  }
  return undefined;
};

export const BrainPlugin = async ({ client, directory }) => ({
  event: async ({ event }) => {
    const sessionID = sessionIdFrom(event);
    if (!sessionID || !isRootSession(event)) return;

    if (event.type === "session.created") {
      const hook = hookPath("claude_session_start_hook.py");
      if (hook) await runHook(hook, { session_id: sessionID, source: "startup" });
      return;
    }

    if (event.type !== "session.idle") return;
    const hook = hookPath("claude_stop_hook.py");
    if (!hook) return;
    const result = await client.session.messages({
      path: { id: sessionID },
      query: { directory },
    });
    const messages = result?.data ?? result ?? [];
    const message = latestAssistantText(messages);
    if (message) await runHook(hook, { session_id: sessionID, last_assistant_message: message });
  },
});
