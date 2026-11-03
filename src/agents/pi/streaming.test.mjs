import assert from "node:assert/strict";
import { test } from "node:test";
import { AgentSession, ExtensionRunner } from "@earendil-works/pi-coding-agent";
import extension from "./extension.ts";

function streamingSession(output) {
  const handlers = new Map();
  const delivered = [];
  const calls = [];

  // Exercise Pi's actual input-handler, custom-message and queue methods. A
  // model-backed AgentSession constructor initializes unrelated tools/auth;
  // supply only the state these methods need and capture their agent boundary.
  const session = Object.assign(Object.create(AgentSession.prototype), {
    _isAgentRunActive: true,
    _eventListeners: [],
    _pendingNextTurnMessages: [],
    _pendingCustomMessages: [],
    _steeringMessages: [],
    _followUpMessages: [],
    _resourceLoader: { getPrompts: () => ({ prompts: [] }) },
    agent: {
      steer: (message) => delivered.push({ behavior: "steer", message }),
      followUp: (message) => delivered.push({ behavior: "followUp", message }),
    },
  });

  extension({
    on: (event, handler) => {
      const registered = handlers.get(event) ?? [];
      registered.push(handler);
      handlers.set(event, registered);
    },
    sendMessage: (message, options) => session.sendCustomMessage(message, options),
  }, async (event, payload) => {
    calls.push({ event, payload });
    return output;
  });

  // The real runner propagates input transforms, including their images, so
  // returning a transformed input is a valid way to fix the regression.
  session._extensionRunner = new ExtensionRunner(
    [{ path: "symposium.ts", handlers }],
    {},
    process.cwd(),
    { getSessionId: () => "streaming-session" },
    {},
  );
  session._extensionRunner.isIdleFn = () => !session.isStreaming;
  return { session, delivered, calls };
}

function text(message) {
  return typeof message.content === "string"
    ? message.content
    : message.content.filter((part) => part.type === "text").map((part) => part.text).join("\n");
}

const prompt = "Please change approach";
const context = "Use the newly selected workspace dependency";
const images = [{ type: "image", data: "aW1hZ2U=", mimeType: "image/png" }];

for (const behavior of ["steer", "followUp"]) {
  async function queueInput(output) {
    const fixture = streamingSession(output);
    assert.equal(await fixture.session[behavior](prompt, images), "queued");
    assert.equal(fixture.calls.length, 1);
    assert.equal(fixture.calls[0].event, "user-prompt-submit");
    assert.equal(fixture.calls[0].payload.prompt, prompt);
    assert.ok(fixture.delivered.every((entry) => entry.behavior === behavior));
    const user = fixture.delivered.find(({ message }) => message.role === "user")?.message;
    assert.ok(user, "the user's input must reach Pi's agent queue");
    assert.ok(text(user).includes(prompt), "the original prompt must be preserved");
    assert.deepEqual(user.content.filter((part) => part.type === "image"), images);
    return fixture;
  }

  test(`${behavior} input without hook context preserves its text and images`, async () => {
    const { delivered } = await queueInput({});
    assert.equal(delivered.length, 1);
    assert.equal(text(delivered[0].message), prompt);
  });

  test(`${behavior} input receives its prompt-hook context while the agent is running`, async () => {
    const { delivered } = await queueInput({ additionalContext: context });
    assert.ok(
      delivered.some(({ message }) => text(message).includes(context)),
      "prompt-hook context must reach the agent with the queued input, without a later idle prompt",
    );
  });
}
