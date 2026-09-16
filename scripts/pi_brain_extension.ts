// Brain lifecycle bridge for pi.
//
// Frontend event details stay here. Brain's existing Python hooks own session
// rotation, attribution, deduplication, and response publication.
//
// Loaded with `pi -e <this file>`, so it runs before project trust is resolved
// and never depends on the workspace being trusted.

import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";

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

const hookEnvironment = () =>
  Object.fromEntries(
    Object.entries(process.env).filter(
      ([name, value]) =>
        value !== undefined &&
        (BRAIN_ENVIRONMENT.includes(name) || RUNTIME_ENVIRONMENT.includes(name)),
    ),
  );

const hookPath = (name) => {
  const root = process.env.BRAIN_ROOT;
  if (!root) return undefined;
  return `${root}/.brain/hooks/${name}`;
};

const runHook = (hook, payload) =>
  new Promise((resolve) => {
    let child;
    try {
      child = spawn("python3", [hook], {
        stdio: ["pipe", "ignore", "ignore"],
        env: hookEnvironment(),
      });
    } catch {
      resolve(false);
      return;
    }
    child.on("error", () => resolve(false));
    child.on("close", (code) => resolve(code === 0));
    child.stdin.on("error", () => {});
    try {
      child.stdin.end(JSON.stringify(payload));
    } catch {
      resolve(false);
    }
  });

const invokeHook = async (ctx, operation, hook, payload) => {
  if (!hook) return false;
  try {
    return await runHook(hook, payload);
  } catch {
    // A lifecycle failure must never surface as a crash inside the session.
    try {
      ctx?.ui?.notify?.(`Brain lifecycle integration failed: ${operation}`, "error");
    } catch {
      // Notification failures cannot safely be recovered inside an event hook.
    }
    return false;
  }
};

// The receiver marks an authenticated prompt with a terminal comment carrying
// the job token Brain issued for that turn. Anything else is ordinary input.
const exactReceiverMarker = (value) => {
  const token = process.env.BRAIN_RECEIVER_JOB_TOKEN;
  if (typeof value !== "string" || typeof token !== "string" || !token) return false;
  const lines = value.replace(/\r?\n$/, "").split(/\r?\n/);
  return lines.at(-1) === `<!-- brain:receiver-job-token=${token} -->`;
};

const assistantText = (message) => {
  if (message?.role !== "assistant") return undefined;
  if (message.stopReason === "error" || message.stopReason === "aborted") return undefined;
  const text = (Array.isArray(message.content) ? message.content : [])
    .filter((block) => block?.type === "text" && typeof block.text === "string")
    .map((block) => block.text)
    .filter((block) => block.length > 0)
    .join("\n\n");
  return text.trim() ? text : undefined;
};

const sessionIdOf = (ctx) => {
  try {
    const id = ctx?.sessionManager?.getSessionId?.();
    return typeof id === "string" && id ? id : undefined;
  } catch {
    return undefined;
  }
};

export default function (pi) {
  // The turn's latest publishable assistant text, cleared once published so a
  // second settle cannot republish the same answer.
  let pendingAnswer;
  // The turn id Brain's observation bridge accepted for the current receiver
  // prompt, so progress observations stay inside one authorized turn.
  let acceptedTurnId;

  pi.on("session_start", async (event, ctx) => {
    const sessionId = sessionIdOf(ctx);
    if (!sessionId) return;
    pendingAnswer = undefined;
    acceptedTurnId = undefined;
    await invokeHook(ctx, "session_start_bridge", hookPath("agent_session_start_hook.py"), {
      session_id: sessionId,
      source: event?.reason ?? "startup",
    });
  });

  pi.on("input", async (event, ctx) => {
    const sessionId = sessionIdOf(ctx);
    acceptedTurnId = undefined;
    pendingAnswer = undefined;
    if (!sessionId || !exactReceiverMarker(event?.text)) return;
    const turnId = randomUUID();
    const accepted = await invokeHook(
      ctx,
      "receiver_acceptance_bridge",
      hookPath("receiver_observation_bridge.py"),
      {
        hook_event_name: "UserPromptSubmit",
        session_id: sessionId,
        turn_id: turnId,
        prompt: event.text,
      },
    );
    if (accepted) acceptedTurnId = turnId;
  });

  pi.on("message_end", (event) => {
    const text = assistantText(event?.message);
    if (text) pendingAnswer = text;
  });

  pi.on("tool_execution_end", async (event, ctx) => {
    const sessionId = sessionIdOf(ctx);
    if (!sessionId || !acceptedTurnId || typeof event?.toolCallId !== "string") return;
    await invokeHook(
      ctx,
      "receiver_progress_bridge",
      hookPath("receiver_observation_bridge.py"),
      {
        hook_event_name: "PostToolUse",
        session_id: sessionId,
        turn_id: acceptedTurnId,
        tool_use_id: event.toolCallId,
      },
    );
  });

  // `agent_settled` is the one event that means pi will not continue on its
  // own: no retry, no compaction, no queued follow-up left.
  pi.on("agent_settled", async (_event, ctx) => {
    const sessionId = sessionIdOf(ctx);
    const answer = pendingAnswer;
    if (!sessionId || !answer) return;
    pendingAnswer = undefined;
    await invokeHook(ctx, "session_stop_bridge", hookPath("agent_session_stop_hook.py"), {
      session_id: sessionId,
      last_assistant_message: answer,
    });
  });
}
