import * as assert from "assert";
import { EventEmitter } from "events";
import { ChildProcess } from "child_process";
import {
  runStartupWatchdog,
  StartupWatchdogContext,
  StartupWatchdogError,
} from "../acpAgentActor";

class FakeChildProcess extends EventEmitter {
  stdout = new EventEmitter();
  killed = false;

  kill(): boolean {
    this.killed = true;
    return true;
  }
}

function toChildProcess(process: FakeChildProcess): ChildProcess {
  return process as unknown as ChildProcess;
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

suite("Startup Watchdog", function () {
  this.timeout(5000);

  test("Should emit slow threshold feedback without failing startup", async () => {
    const process = new FakeChildProcess();
    const slowContexts: StartupWatchdogContext[] = [];

    const watched = runStartupWatchdog({
      phase: "initialize",
      command: "symposium-acp-agent",
      args: ["run"],
      process: toChildProcess(process),
      slowThresholdMs: 15,
      hardTimeoutMs: 200,
      initialize: async () => {
        await wait(40);
        return "ok";
      },
      onSlowThreshold: (context) => {
        slowContexts.push(context);
      },
    });

    const result = await watched;
    assert.strictEqual(result, "ok");
    assert.strictEqual(slowContexts.length, 1);
    assert.strictEqual(slowContexts[0].phase, "initialize");
    assert.ok(slowContexts[0].elapsedMs >= 15);

    await wait(220);
    assert.strictEqual(process.killed, false);
  });

  test("Should hard-timeout and allow caller to abort startup", async () => {
    const process = new FakeChildProcess();
    let hardTimeoutCount = 0;

    const watched = runStartupWatchdog({
      phase: "initialize",
      command: "symposium-acp-agent",
      args: ["run"],
      process: toChildProcess(process),
      slowThresholdMs: 100,
      hardTimeoutMs: 25,
      initialize: async () => {
        await new Promise<void>(() => {});
      },
      onHardTimeout: () => {
        hardTimeoutCount += 1;
        process.kill();
      },
    });

    await assert.rejects(watched, (error: unknown) => {
      assert.ok(error instanceof StartupWatchdogError);
      assert.strictEqual(error.diagnostics.reason, "hard_timeout");
      assert.strictEqual(error.diagnostics.phase, "initialize");
      assert.strictEqual(error.diagnostics.hardTimeoutMs, 25);
      return true;
    });

    assert.strictEqual(hardTimeoutCount, 1);
    assert.strictEqual(process.killed, true);
  });

  test("Should fail immediately when process exits during initialize", async () => {
    const process = new FakeChildProcess();
    let hardTimeoutCount = 0;

    const watched = runStartupWatchdog({
      phase: "initialize",
      command: "symposium-acp-agent",
      args: ["run"],
      process: toChildProcess(process),
      slowThresholdMs: 120,
      hardTimeoutMs: 250,
      initialize: async () => {
        await new Promise<void>(() => {});
      },
      onHardTimeout: () => {
        hardTimeoutCount += 1;
      },
    });

    setTimeout(() => {
      process.emit("exit", 17, null);
    }, 10);

    await assert.rejects(watched, (error: unknown) => {
      assert.ok(error instanceof StartupWatchdogError);
      assert.strictEqual(error.diagnostics.reason, "process_exit");
      assert.strictEqual(error.diagnostics.exitCode, 17);
      assert.strictEqual(error.diagnostics.signal, null);
      assert.ok(error.diagnostics.elapsedMs < 250);
      return true;
    });

    await wait(280);
    assert.strictEqual(hardTimeoutCount, 0);
  });

  test("Should fail immediately when stdout closes during initialize", async () => {
    const process = new FakeChildProcess();
    let hardTimeoutCount = 0;

    const watched = runStartupWatchdog({
      phase: "initialize",
      command: "symposium-acp-agent",
      args: ["run"],
      process: toChildProcess(process),
      slowThresholdMs: 120,
      hardTimeoutMs: 250,
      initialize: async () => {
        await new Promise<void>(() => {});
      },
      onHardTimeout: () => {
        hardTimeoutCount += 1;
      },
    });

    setTimeout(() => {
      process.stdout.emit("close");
    }, 10);

    await assert.rejects(watched, (error: unknown) => {
      assert.ok(error instanceof StartupWatchdogError);
      assert.strictEqual(error.diagnostics.reason, "stdout_close");
      assert.strictEqual(error.diagnostics.stream, "stdout");
      assert.ok(error.diagnostics.elapsedMs < 250);
      return true;
    });

    await wait(280);
    assert.strictEqual(hardTimeoutCount, 0);
  });

  test("Should include initialize-error diagnostics when initialize rejects", async () => {
    const process = new FakeChildProcess();
    let hardTimeoutCount = 0;

    const watched = runStartupWatchdog({
      phase: "initialize",
      command: "symposium-acp-agent",
      args: ["run"],
      process: toChildProcess(process),
      slowThresholdMs: 120,
      hardTimeoutMs: 250,
      initialize: async () => {
        throw new Error("handshake failed");
      },
      onHardTimeout: () => {
        hardTimeoutCount += 1;
      },
    });

    await assert.rejects(watched, (error: unknown) => {
      assert.ok(error instanceof StartupWatchdogError);
      assert.strictEqual(error.diagnostics.reason, "initialize_error");
      assert.strictEqual(error.diagnostics.errorMessage, "handshake failed");
      return true;
    });

    await wait(280);
    assert.strictEqual(hardTimeoutCount, 0);
  });
});
