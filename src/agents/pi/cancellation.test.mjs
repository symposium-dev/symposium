import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { test } from "node:test";
import { cargoAgentsBinary } from "./cli-test-helpers.mjs";
import { runHook } from "./extension.ts";

const unixOnly = process.platform === "win32" ? "Uses Unix process signals and a POSIX shell" : false;

async function waitUntil(ready, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (!ready() && Date.now() < deadline) await sleep(5);
  return ready();
}

function isRunning(pid) {
  try {
    process.kill(pid, 0); // Observe only; never signal a possibly reused PID.
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

async function startGatedHook(t, timeout = 10_000) {
  const root = mkdtempSync(join(tmpdir(), "symposium-pi-cancel-"));
  const config = join(root, "symposium");
  const plugin = join(config, "plugins", "cancel-probe");
  const controller = new AbortController();
  let pending;
  let pluginPid;
  const savedHome = process.env.SYMPOSIUM_HOME;

  // Top-level tests in this file run sequentially, and Node's test runner gives
  // each test file its own process. Restore the environment even on assertion
  // failure. Keep the fixture until its surviving child has acknowledged cleanup.
  t.after(async () => {
    writeFileSync(join(root, "cleanup"), "");
    controller.abort();
    if (savedHome === undefined) delete process.env.SYMPOSIUM_HOME;
    else process.env.SYMPOSIUM_HOME = savedHome;
    try {
      await pending;
      if (pluginPid !== undefined) {
        assert.ok(await waitUntil(() => existsSync(join(root, "stopped")) || !isRunning(pluginPid)),
          "the fixture's plugin process must stop during cleanup");
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  mkdirSync(plugin, { recursive: true });
  writeFileSync(join(config, "config.toml"), `auto-update = "off"
auto-sync = false
[defaults]
symposium-recommendations = false
user-plugins = true
`);
  const script = join(plugin, "probe.sh");
  writeFileSync(join(plugin, "Symposium.toml"), `name = "cancel-probe"
depends-on = ["*"]
[[hooks]]
name = "cancel-probe"
event = "PreToolUse"
matcher = "bash"
format = "pi"
command = { script = ${JSON.stringify(script)} }
`);
  const quotedRoot = `'${root.replaceAll("'", "'\\''")}'`;
  writeFileSync(script, `#!/bin/sh
cd ${quotedRoot} || exit 1
trap ': > stopped' EXIT
echo "$PPID" > cli-pid
echo "$$" > plugin-pid
# A nested child that ignores TERM must not outlive the dispatch either.
(
  trap '' TERM
  : > descendant-started
  j=0
  while [ ! -f release ]; do
    [ ! -f cleanup ] || exit 0
    j=$((j+1)); [ $j -lt 2000 ] || exit 1
    sleep 0.01
  done
  [ ! -f cleanup ] || exit 0
  : > descendant-completed
) &
: > started
i=0
while [ ! -f release ]; do
  [ ! -f cleanup ] || exit 0
  i=$((i+1)); [ $i -lt 2000 ] || exit 1
  sleep 0.01
done
[ ! -f cleanup ] || exit 0
: > completed
while [ ! -f cleanup ] && [ $i -lt 2000 ]; do
  i=$((i+1))
  sleep 0.01
done
`);

  process.env.SYMPOSIUM_HOME = config;
  pending = runHook("pre-tool-use", {
    cwd: root, session_id: "cancel-probe", tool_name: "bash", tool_input: { command: "true" },
  }, root, cargoAgentsBinary(), controller.signal, timeout)
    .then(value => ({ value }), error => ({ error }));
  assert.ok(await waitUntil(() => existsSync(join(root, "started"))), "the real CLI must start the plugin");
  assert.ok(await waitUntil(() => existsSync(join(root, "descendant-started"))), "the nested child must start");
  pluginPid = Number(readFileSync(join(root, "plugin-pid"), "utf8"));
  const cliPid = Number(readFileSync(join(root, "cli-pid"), "utf8"));
  assert.ok(Number.isInteger(pluginPid) && pluginPid > 0);
  assert.ok(Number.isInteger(cliPid) && cliPid > 0);
  assert.equal(existsSync(join(root, "completed")), false, "the gate prevents early side effects");
  return { root, controller, pending, cliPid };
}

test("an uncancelled Pi hook completes when its plugin gate opens", { skip: unixOnly }, async t => {
  const { root, pending } = await startGatedHook(t);
  writeFileSync(join(root, "release"), "");
  assert.ok(await waitUntil(() => existsSync(join(root, "completed"))), "the control must perform its side effect");
  assert.ok(await waitUntil(() => existsSync(join(root, "descendant-completed"))), "the nested control must run");
  writeFileSync(join(root, "cleanup"), "");
  const result = await pending;
  assert.equal(result.error, undefined);
  assert.deepEqual(result.value, {});
});

test("timing out a real Pi hook stops its plugin before more side effects", { skip: unixOnly }, async t => {
  const { root, pending, cliPid } = await startGatedHook(t, 2000);
  const result = await pending;
  assert.match(result.error?.message, /timed out after 2000 ms/);
  assert.ok(await waitUntil(() => !isRunning(cliPid)));
  writeFileSync(join(root, "release"), "");
  assert.equal(await waitUntil(() => existsSync(join(root, "completed")), 500), false);
  assert.equal(existsSync(join(root, "descendant-completed")), false);
});

test("cancelling a real Pi hook stops its plugin before more side effects", { skip: unixOnly }, async t => {
  const { root, controller, pending, cliPid } = await startGatedHook(t);
  controller.abort();
  const result = await pending;
  assert.ok(result.error instanceof Error, "the aborted hook must reject");
  assert.ok(await waitUntil(() => !isRunning(cliPid)), "cargo-agents must exit after cancellation");

  // Permit the write only after cancellation has returned AND the real CLI has
  // exited. A surviving plugin cannot hide behind startup or shutdown timing.
  writeFileSync(join(root, "release"), "");
  assert.equal(await waitUntil(() => existsSync(join(root, "completed")), 500), false,
    "the plugin must not keep writing after the Pi hook is cancelled");
  assert.equal(existsSync(join(root, "descendant-completed")), false,
    "a nested child that ignores SIGTERM must also stop");
});
