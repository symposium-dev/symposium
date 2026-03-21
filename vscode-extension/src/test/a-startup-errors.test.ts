import * as assert from "assert";
import * as vscode from "vscode";

const POLL_INTERVAL_MS = 20;
const WAIT_TIMEOUT_MS = 5000;

async function waitFor<T>(
  description: string,
  check: () => Promise<T | undefined>,
  timeoutMs: number = WAIT_TIMEOUT_MS,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const value = await check();
    if (value !== undefined) {
      return value;
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }

  throw new Error(`Timed out waiting for ${description}`);
}

suite("Startup Error Routing Tests", () => {
  test("Startup failures should queue agent-error messages", async function () {
    this.timeout(20000);

    const config = vscode.workspace.getConfiguration("symposium");
    const originalAcpAgentPath = config.get<string>("acpAgentPath", "");
    const missingBinaryPath = `/tmp/symposium-missing-${Date.now()}`;
    const tabId = `test-tab-startup-error-${Date.now()}`;

    try {
      await config.update(
        "acpAgentPath",
        missingBinaryPath,
        vscode.ConfigurationTarget.Global,
      );

      const extension = vscode.extensions.getExtension(
        "symposium-dev.symposium",
      );
      assert.ok(extension);
      await extension.activate();

      await vscode.commands.executeCommand("symposium.chatView.focus");
      await vscode.commands.executeCommand("symposium.test.resetActors");

      await vscode.commands.executeCommand("symposium.test.simulateNewTab", tabId);
      const startupError = await waitFor("startup agent-error message", async () => {
        const queuedMessages = (await vscode.commands.executeCommand(
          "symposium.test.getQueuedMessages",
          tabId,
        )) as Array<{ type: string; error?: unknown }>;

        return queuedMessages.find((message) => message.type === "agent-error");
      });

      assert.ok(
        startupError,
        "Startup failures should be delivered as agent-error messages",
      );

      const errorPayload = startupError.error;
      assert.ok(
        errorPayload !== undefined,
        "agent-error should include an error payload",
      );

      if (typeof errorPayload === "string") {
        assert.ok(
          errorPayload.includes("Failed to initialize chat session"),
          "String error payload should include startup context",
        );
      } else {
        const structured = errorPayload as Record<string, unknown>;
        assert.strictEqual(
          structured.context,
          "Failed to initialize chat session",
        );
        assert.strictEqual(
          typeof structured.message,
          "string",
          "Structured error payload should include a user-visible message",
        );
        assert.ok(
          !Object.prototype.hasOwnProperty.call(structured, "details"),
          "Structured error payload should not include diagnostic details",
        );
      }
    } finally {
      await config.update(
        "acpAgentPath",
        originalAcpAgentPath,
        vscode.ConfigurationTarget.Global,
      );
      await vscode.commands.executeCommand("symposium.test.resetActors");
    }
  });
});
