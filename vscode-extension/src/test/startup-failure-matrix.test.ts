import * as assert from "assert";
import * as path from "path";
import * as vscode from "vscode";
import { logger } from "../extension";
import { LogEvent } from "../logger";

type StartupScenario = "exit" | "hang" | "close" | "acp-error";

interface QueuedMessage {
  index: number;
  type: string;
  tabId: string;
  error?: unknown;
  [key: string]: unknown;
}

const POLL_INTERVAL_MS = 20;
const WAIT_TIMEOUT_MS = 6000;
const STARTUP_SLOW_THRESHOLD_MS = 100;
const STARTUP_HARD_TIMEOUT_MS = 300;

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function getAgentErrorMessage(errorPayload: unknown): string {
  if (typeof errorPayload === "string") {
    return errorPayload;
  }

  if (isRecord(errorPayload) && typeof errorPayload.message === "string") {
    return errorPayload.message;
  }

  return JSON.stringify(errorPayload);
}

function getWatchdogReason(logData: unknown): string | undefined {
  if (!isRecord(logData)) {
    return undefined;
  }

  return typeof logData.reason === "string" ? logData.reason : undefined;
}

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

async function getQueuedMessages(tabId: string): Promise<QueuedMessage[]> {
  const messages = (await vscode.commands.executeCommand(
    "symposium.test.getQueuedMessages",
    tabId,
  )) as QueuedMessage[];
  return messages ?? [];
}

suite("Startup Failure Matrix Integration", () => {
  let logDisposable: vscode.Disposable;
  const logEvents: LogEvent[] = [];
  let fakeAgentPath: string;
  const originalSettings = new Map<string, unknown>();
  const trackedSettingKeys = [
    "acpAgentPath",
    "proxySpawnArgs",
    "startupSlowThresholdMs",
    "startupHardTimeoutMs",
    "logLevel",
  ] as const;

  async function setSetting(key: string, value: unknown): Promise<void> {
    await vscode.workspace
      .getConfiguration("symposium")
      .update(key, value, vscode.ConfigurationTarget.Global);
  }

  async function configureScenario(scenario: StartupScenario): Promise<void> {
    await vscode.commands.executeCommand("symposium.test.resetState");
    await setSetting("acpAgentPath", fakeAgentPath);
    await setSetting("proxySpawnArgs", [`--startup-scenario=${scenario}`]);
    await setSetting("startupSlowThresholdMs", STARTUP_SLOW_THRESHOLD_MS);
    await setSetting("startupHardTimeoutMs", STARTUP_HARD_TIMEOUT_MS);
  }

  async function createScenarioTab(scenario: StartupScenario): Promise<string> {
    const tabId = `startup-${scenario}-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
    await vscode.commands.executeCommand("symposium.chatView.focus");
    await vscode.commands.executeCommand("symposium.test.simulateNewTab", tabId);
    return tabId;
  }

  async function waitForLogEvent(
    description: string,
    predicate: (event: LogEvent) => boolean,
  ): Promise<LogEvent> {
    return waitFor(description, async () => {
      return logEvents.find(predicate);
    });
  }

  async function waitForQueuedMessage(
    tabId: string,
    description: string,
    predicate: (message: QueuedMessage) => boolean,
  ): Promise<QueuedMessage> {
    return waitFor(description, async () => {
      const queue = await getQueuedMessages(tabId);
      return queue.find(predicate);
    });
  }

  suiteSetup(async function () {
    this.timeout(20000);

    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension, "Symposium extension should be installed");
    await extension.activate();
    await vscode.commands.executeCommand("symposium.chatView.focus");

    fakeAgentPath = path.resolve(
      __dirname,
      "../../test-fixtures/fake-startup-agent.cjs",
    );

    const config = vscode.workspace.getConfiguration("symposium");
    for (const key of trackedSettingKeys) {
      originalSettings.set(key, config.get(key));
    }

    await setSetting("logLevel", "debug");
    logDisposable = logger.onLog((event) => {
      logEvents.push(event);
    });
  });

  suiteTeardown(async function () {
    this.timeout(20000);

    logDisposable.dispose();
    await vscode.commands.executeCommand("symposium.test.resetState");

    for (const [key, value] of originalSettings.entries()) {
      await setSetting(key, value);
    }
  });

  setup(() => {
    logEvents.length = 0;
  });

  test("exit scenario logs stderr and enqueues agent-error", async function () {
    this.timeout(20000);

    await configureScenario("exit");
    const tabId = await createScenarioTab("exit");

    await waitForLogEvent("exit stderr log", (event) => {
      return (
        event.category === "agent-stderr" &&
        event.message.includes("startup-scenario=exit")
      );
    });

    const agentError = await waitForQueuedMessage(
      tabId,
      "exit agent-error message",
      (message) => message.type === "agent-error",
    );

    const watchdogFailure = await waitForLogEvent(
      "startup watchdog failure log",
      (event) =>
        event.category === "agent" &&
        event.message === "ACP startup watchdog failure",
    );

    const startupReason = getWatchdogReason(watchdogFailure.data);
    assert.ok(
      startupReason === "process-exit" ||
        startupReason === "stdout-close" ||
        startupReason === "hard-timeout",
      `Expected process-exit, stdout-close, or hard-timeout, got ${startupReason}`,
    );
    assert.ok(
      getAgentErrorMessage(agentError.error).includes("ACP startup failed"),
      "Expected startup failure summary in user-visible agent-error message",
    );
  });

  test("hang scenario emits slow warning before hard-timeout agent-error", async function () {
    this.timeout(20000);

    await configureScenario("hang");
    const tabId = await createScenarioTab("hang");

    const slowWarning = await waitForLogEvent("slow-start warning", (event) => {
      return (
        event.category === "agent" &&
        event.message === "[WARN] ACP startup exceeded slow threshold"
      );
    });

    const watchdogFailure = await waitForLogEvent(
      "startup watchdog failure log",
      (event) =>
        event.category === "agent" &&
        event.message === "ACP startup watchdog failure",
    );

    const slowChatFeedback = await waitForLogEvent(
      "slow-start chat feedback",
      (event) =>
        event.category === "webview" &&
        event.message === "Sending message to webview" &&
        isRecord(event.data) &&
        event.data.tabId === tabId &&
        event.data.messageType === "agent-startup-slow",
    );

    const agentError = await waitForQueuedMessage(
      tabId,
      "hang agent-error message",
      (message) => message.type === "agent-error",
    );

    assert.strictEqual(getWatchdogReason(watchdogFailure.data), "hard-timeout");
    assert.ok(
      getAgentErrorMessage(agentError.error).includes("ACP startup failed"),
      "Expected startup failure summary in user-visible agent-error message",
    );
    assert.ok(
      slowWarning.timestamp.getTime() <= watchdogFailure.timestamp.getTime(),
      "Slow threshold feedback should be logged before hard-timeout failure",
    );
    assert.ok(
      slowChatFeedback.timestamp.getTime() <= watchdogFailure.timestamp.getTime(),
      "Slow threshold chat feedback should appear before hard-timeout failure",
    );
  });

  test("close scenario enqueues stdout-close agent-error", async function () {
    this.timeout(20000);

    await configureScenario("close");
    const tabId = await createScenarioTab("close");

    const agentError = await waitForQueuedMessage(
      tabId,
      "close agent-error message",
      (message) => message.type === "agent-error",
    );

    const watchdogFailure = await waitForLogEvent(
      "startup watchdog failure log",
      (event) =>
        event.category === "agent" &&
        event.message === "ACP startup watchdog failure",
    );

    assert.strictEqual(getWatchdogReason(watchdogFailure.data), "stdout-close");
    assert.ok(
      getAgentErrorMessage(agentError.error).includes("ACP startup failed"),
      "Expected startup failure summary in user-visible agent-error message",
    );
  });

  test("acp-error scenario enqueues initialize-rejected agent-error", async function () {
    this.timeout(20000);

    await configureScenario("acp-error");
    const tabId = await createScenarioTab("acp-error");

    const agentError = await waitForQueuedMessage(
      tabId,
      "acp-error agent-error message",
      (message) => message.type === "agent-error",
    );

    const watchdogFailure = await waitForLogEvent(
      "startup watchdog failure log",
      (event) =>
        event.category === "agent" &&
        event.message === "ACP startup watchdog failure",
    );

    assert.strictEqual(
      getWatchdogReason(watchdogFailure.data),
      "initialize-rejected",
    );
    assert.ok(
      getAgentErrorMessage(agentError.error).includes(
        "simulated acp initialize error",
      ) ||
        getAgentErrorMessage(agentError.error).includes("ACP startup failed"),
      "Expected initialize rejection details in user-visible error",
    );
  });
});
