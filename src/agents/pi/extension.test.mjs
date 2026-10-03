import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { loadSkills, loadSkillsFromDir, ProjectTrustStore } from "@earendil-works/pi-coding-agent";
import extension, { DEFAULT_HOOK_TIMEOUT_MS, managedSkillPaths, runHook } from "./extension.ts";

function setup(t, output = {}) {
  const handlers = new Map();
  const messages = [];
  const calls = [];
  const warnings = [];
  const ctx = {
    cwd: "/project", sessionManager: { getSessionId: () => "session-1" },
    isProjectTrusted: () => true, hasUI: true,
    ui: { notify: (message, level) => warnings.push({ message, level }) },
  };
  const dispatch = async (event, payload, cwd) => {
    calls.push({ event, payload, cwd });
    if (output instanceof Error) throw output;
    return typeof output === "function" ? output(event, payload, cwd) : output;
  };
  extension({
    on: (event, handler) => handlers.set(event, handler),
    sendMessage: (...args) => messages.push(args),
  }, dispatch);
  return { handlers, messages, calls, ctx, warnings };
}

test("the default hook timeout allows ten minutes for cold installations", () => {
  assert.equal(DEFAULT_HOOK_TIMEOUT_MS, 600_000);
});

test("session start syncs before resources are discovered and adds context", async (t) => {
  const { handlers, messages, calls, ctx } = setup(t, { additionalContext: "startup context" });
  await handlers.get("session_start")({ reason: "startup" }, ctx);
  assert.deepEqual(calls, [{
    event: "session-start", payload: { cwd: "/project", session_id: "session-1" }, cwd: "/project",
  }]);
  assert.equal(messages[0][0].content, "startup context");
  assert.deepEqual(messages[0][1], { deliverAs: "nextTurn" });
  assert.ok(handlers.has("resources_discover"));
});

test("prompt and stop events retain context without starting another turn", async (t) => {
  const { handlers, messages, calls, ctx } = setup(t, { additionalContext: "context" });
  await handlers.get("input")({ text: "hello", source: "interactive" }, ctx);
  await handlers.get("agent_end")({}, ctx);
  assert.equal(calls[0].event, "user-prompt-submit");
  assert.equal(calls[0].payload.prompt, "hello");
  assert.equal(calls[1].event, "stop");
  assert.ok(messages.every(([, options]) => !options.triggerTurn));
});

test("untrusted projects do not sync, dispatch hooks, or expose managed skills", async (t) => {
  const { handlers, messages, calls, ctx } = setup(t);
  ctx.isProjectTrusted = () => false;
  for (const event of ["session_start", "input", "tool_call", "tool_result", "agent_end"]) {
    await handlers.get(event)({}, ctx);
  }
  assert.deepEqual(handlers.get("resources_discover")({ cwd: ctx.cwd }, ctx), { skillPaths: [] });
  assert.deepEqual(calls, []);
  assert.deepEqual(messages, []);
});

test("pre-tool-use maps denial to a blocked Pi tool", async (t) => {
  const { handlers, ctx, calls } = setup(t, { decision: "deny", additionalContext: "not allowed" });
  const event = { toolCallId: "deny-1", toolName: "bash", input: { command: "rm -rf /" } };
  assert.deepEqual(await handlers.get("tool_call")(event, ctx), { block: true, reason: "not allowed" });
  assert.equal(calls[0].payload.tool_name, "bash");
  assert.deepEqual(calls[0].payload.tool_input, event.input);
});

test("pre-tool-use replaces inputs and delivers context with the matching result", async (t) => {
  const { handlers, messages, ctx } = setup(t, (event) => event === "pre-tool-use"
    ? { updatedInput: { command: "pwd" }, additionalContext: "safer input" } : {});
  const input = { command: "ls", timeout: 5 };
  const event = { toolCallId: "rewrite-1", toolName: "bash", input };
  await handlers.get("tool_call")(event, ctx);
  assert.deepEqual(input, { command: "pwd" });
  assert.deepEqual(messages, [], "pre-tool context must not wait for another prompt");
  const result = await handlers.get("tool_result")({
    ...event, content: [{ type: "text", text: "result" }], isError: false,
  }, ctx);
  assert.deepEqual(result.content, [
    { type: "text", text: "result" }, { type: "text", text: "safer input" },
  ]);
  assert.deepEqual(messages, []);
});

test("invalid updated inputs and hook failures warn without blocking tools", async (t) => {
  for (const output of [{ updatedInput: [] }, new Error("hook failed")]) {
    const { handlers, ctx, warnings } = setup(t, output);
    const input = { command: "pwd" };
    const result = await handlers.get("tool_call")({ toolCallId: "error-1", toolName: "bash", input }, ctx);
    assert.equal(result, undefined);
    assert.deepEqual(input, { command: "pwd" });
    assert.equal(warnings.length, 1);
    assert.equal(warnings[0].level, "warning");
  }
});

test("hook failures warn once across events and turns, then reset for a new session", async (t) => {
  const { handlers, ctx, warnings } = setup(t, new Error("missing executable"));
  const event = { toolCallId: "warning-1", toolName: "bash", input: {}, content: [], isError: false };
  await handlers.get("session_start")({}, ctx);
  for (let turn = 0; turn < 2; turn++) {
    await handlers.get("input")({ text: "hello" }, ctx);
    assert.equal(await handlers.get("tool_call")(event, ctx), undefined);
    await handlers.get("tool_result")(event, ctx);
    await handlers.get("agent_end")({}, ctx);
  }
  assert.equal(warnings.length, 1);
  ctx.sessionManager.getSessionId = () => "session-2";
  await handlers.get("session_start")({}, ctx);
  assert.equal(warnings.length, 2);
});

test("aborted hooks are silent and do not consume the session warning", async (t) => {
  for (const abortBeforeDispatch of [false, true]) {
    const controller = new AbortController();
    if (abortBeforeDispatch) controller.abort();
    const { handlers, ctx, warnings } = setup(t, () => {
      controller.abort();
      throw new Error("request aborted");
    });
    ctx.signal = controller.signal;
    const event = { toolCallId: "abort-1", toolName: "bash", input: { command: "pwd" } };
    assert.equal(await handlers.get("tool_call")(event, ctx), undefined);
    assert.deepEqual(event.input, { command: "pwd" });
    assert.deepEqual(warnings, []);

    // A later error without cancellation must still produce the first warning.
    ctx.signal = new AbortController().signal;
    await handlers.get("input")({ text: "retry" }, ctx);
    assert.equal(warnings.length, 1);
    await handlers.get("agent_end")({}, ctx);
    assert.equal(warnings.length, 1);
  }
});

test("hook errors warn on stderr only once when Pi has no UI", async (t) => {
  const warnings = [];
  t.mock.method(console, "warn", (message) => warnings.push(message));
  const { handlers, ctx } = setup(t, new Error("bad configuration"));
  ctx.hasUI = false;
  assert.equal(await handlers.get("tool_call")({ toolCallId: "stderr-1", toolName: "bash", input: {} }, ctx), undefined);
  await handlers.get("input")({ text: "hello" }, ctx);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /bad configuration/);
});

test("a failed warning notification cannot block a tool", async (t) => {
  const warnings = [];
  t.mock.method(console, "warn", (message) => warnings.push(message));
  const { handlers, ctx } = setup(t, new Error("missing executable"));
  ctx.ui.notify = () => { throw new Error("notification failed"); };
  assert.equal(await handlers.get("tool_call")({ toolCallId: "ui-1", toolName: "bash", input: {} }, ctx), undefined);
  assert.match(warnings[0], /missing executable/);
});

test("post-tool-use preserves result data and adds context", async (t) => {
  const { handlers, calls, ctx } = setup(t, { additionalContext: "check the result" });
  const event = {
    toolCallId: "post-1", toolName: "custom", input: { value: 1 }, content: [{ type: "text", text: "result" }],
    details: { value: 2 }, structuredContent: { value: 2 }, isError: true,
  };
  const result = await handlers.get("tool_result")(event, ctx);
  assert.deepEqual(result.content, [...event.content, { type: "text", text: "check the result" }]);
  assert.deepEqual(result.structuredContent, event.structuredContent);
  assert.deepEqual(calls[0].payload.tool_response, {
    content: event.content, details: event.details, structuredContent: event.structuredContent, isError: true,
  });
});

test("pre- and post-tool context are joined even when a post hook fails", async (t) => {
  for (const fail of [false, true]) {
    const { handlers, ctx, warnings, messages } = setup(t, (event) => {
      if (event === "pre-tool-use") return { additionalContext: "before" };
      if (fail) throw new Error("post hook failed");
      return { additionalContext: "after" };
    });
    const event = {
      toolCallId: "join-1", toolName: "custom", input: {},
      content: [{ type: "text", text: "result" }], structuredContent: { value: 2 }, isError: true,
    };
    await handlers.get("tool_call")(event, ctx);
    const result = await handlers.get("tool_result")(event, ctx);
    assert.deepEqual(result.content.map((c) => c.text), fail ? ["result", "before"] : ["result", "before", "after"]);
    assert.deepEqual(result.structuredContent, event.structuredContent);
    assert.equal(event.isError, true);
    assert.deepEqual(messages, []);
    assert.equal(warnings.length, fail ? 1 : 0);
    // Reusing an ID after its result must not reuse old pre-tool context.
    const next = await handlers.get("tool_result")(event, ctx);
    assert.ok(!next?.content.some((c) => c.text === "before"));
  }
});

test("unfinished pre-tool context is cleared at turn and session boundaries", async (t) => {
  for (const boundary of ["agent_end", "session_start", "session_shutdown"]) {
    const { handlers, ctx } = setup(t, (event) => event === "pre-tool-use" ? { additionalContext: "stale" } : {});
    const event = { toolCallId: "reused-1", toolName: "bash", input: {}, content: [], isError: false };
    await handlers.get("tool_call")(event, ctx);
    await handlers.get(boundary)({}, ctx);
    assert.equal(await handlers.get("tool_result")(event, ctx), undefined);
  }
});

test("parallel tool hooks keep their inputs and decisions separate", async () => {
  const handlers = new Map();
  extension({ on: (name, fn) => handlers.set(name, fn), sendMessage() {} },
    async (_event, payload) => {
      await new Promise((resolve) => setImmediate(resolve));
      return payload.tool_input.command === "blocked" ? { decision: "deny" } : {};
    });
  const ctx = { cwd: "/project", sessionManager: { getSessionId: () => "s" }, isProjectTrusted: () => true };
  const results = await Promise.all(["allowed", "blocked"].map((command) =>
    handlers.get("tool_call")({ toolCallId: command, toolName: "bash", input: { command } }, ctx)));
  assert.equal(results[0], undefined);
  assert.equal(results[1].block, true);
});

test("parallel tools attach pre-tool context only to their own results", async (t) => {
  const { handlers, ctx, messages } = setup(t, async (event, payload) => {
    await new Promise((resolve) => setImmediate(resolve));
    return event === "pre-tool-use" ? { additionalContext: `context: ${payload.tool_input.command}` } : {};
  });
  const events = ["first", "second"].map((command) => ({
    toolCallId: command, toolName: "bash", input: { command }, content: [], isError: false,
  }));
  await Promise.all(events.map((event) => handlers.get("tool_call")(event, ctx)));
  const results = await Promise.all(events.toReversed().map((event) => handlers.get("tool_result")(event, ctx)));
  assert.deepEqual(results.map((result) => result.content[0].text), ["context: second", "context: first"]);
  assert.deepEqual(messages, []);
});

function temp(t) {
  const root = mkdtempSync(join(tmpdir(), "symposium-pi-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test("Pi loads generated, gitignored skills through explicit file paths", (t) => {
  const root = temp(t);
  mkdirSync(join(root, ".git"));
  const parent = join(root, ".agents", "skills");
  const managed = join(parent, "managed");
  mkdirSync(managed, { recursive: true });
  writeFileSync(join(managed, "SKILL.md"), "---\nname: managed\ndescription: Managed test skill\n---\nInstructions\n");
  writeFileSync(join(managed, ".symposium"), "");
  writeFileSync(join(managed, ".gitignore"), "*\n");
  const user = join(parent, "user");
  mkdirSync(user);
  writeFileSync(join(user, "SKILL.md"), "---\nname: user\ndescription: User test skill\n---\n");
  const member = join(root, "member", "src");
  mkdirSync(member, { recursive: true });
  const paths = managedSkillPaths(member, root);
  assert.deepEqual(paths, [join(managed, "SKILL.md")]);
  assert.ok(!loadSkillsFromDir({ dir: parent, source: "project" }).skills.some((s) => s.name === "managed"));
  const loaded = loadSkills({ cwd: member, agentDir: join(root, "agent"), skillPaths: paths, includeDefaults: false });
  assert.deepEqual(loaded.skills.map((s) => s.name), ["managed"]);
  assert.deepEqual(loaded.diagnostics, []);
  rmSync(managed, { recursive: true });
  assert.deepEqual(managedSkillPaths(member, root), []);
});

test("resource discovery stops at the repository boundary", (t) => {
  const root = temp(t);
  const outside = join(root, ".agents", "skills", "outside");
  mkdirSync(outside, { recursive: true });
  writeFileSync(join(outside, ".symposium"), "");
  writeFileSync(join(outside, "SKILL.md"), "outside");
  const repo = join(root, "repo");
  mkdirSync(join(repo, ".git"), { recursive: true });
  assert.deepEqual(managedSkillPaths(repo, join(root, "home")), []);
});

test("Pi reads the supported user and project MCP files", (t) => {
  const root = temp(t);
  const agentDir = join(root, "agent");
  mkdirSync(agentDir);
  mkdirSync(join(root, ".pi"));
  new ProjectTrustStore(agentDir).set(root, true);
  const server = join(root, "server.cjs");
  writeFileSync(server, `
    require("node:readline").createInterface({ input: process.stdin }).on("line", line => {
      const request = JSON.parse(line);
      if (request.id === undefined) return;
      const result = request.method === "initialize"
        ? { protocolVersion: request.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "smoke", version: "1" } }
        : { tools: [] };
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }) + "\\n");
    });
  `);
  const config = (name) => JSON.stringify({ mcpServers: {
    [name]: { command: process.execPath, args: [server] },
  } });
  writeFileSync(join(agentDir, "mcp.json"), config("global-smoke"));
  writeFileSync(join(root, ".pi", "mcp.json"), config("project-smoke"));
  const cli = join(dirname(fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"))), "cli.js");
  const result = spawnSync(process.execPath, [cli, "mcp", "list"], {
    cwd: root, env: { ...process.env, PI_CODING_AGENT_DIR: agentDir }, encoding: "utf8", timeout: 20_000,
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /global-smoke/);
  assert.match(result.stdout, /project-smoke/);
});

test("runHook sends stdin without shell interpolation", async (t) => {
  const root = temp(t);
  writeFileSync(join(root, "hook"), `
    let input = "";
    process.stdin.on("data", chunk => input += chunk);
    process.stdin.on("end", () => process.stdout.write(JSON.stringify({
      additionalContext: input, args: process.argv.slice(2)
    })));
  `);
  const payload = { prompt: '$(echo unsafe); "quoted"\\nline' };
  const output = await runHook("user-prompt-submit", payload, root, process.execPath);
  assert.deepEqual(JSON.parse(output.additionalContext), payload);
  assert.deepEqual(output.args, ["pi", "user-prompt-submit"]);
});

test("missing commands, non-zero exits, malformed output, and timeouts do not block tools", async (t) => {
  const root = temp(t);
  for (const script of [
    null,
    'process.stderr.write("bad config"); process.exit(1);',
    'process.stderr.write("plugin error"); process.exit(2);',
    'process.stdout.write("not JSON");',
    'setInterval(() => {}, 1000);',
  ]) {
    if (script !== null) writeFileSync(join(root, "hook"), script);
    const command = script === null ? join(root, "missing") : process.execPath;
    const { handlers, ctx, warnings } = setup(t, (event, payload) =>
      runHook(event, payload, root, command, undefined, script?.startsWith("setInterval") ? 40 : 5000));
    const input = { command: "pwd" };
    assert.equal(await handlers.get("tool_call")({ toolCallId: "process-1", toolName: "bash", input }, ctx), undefined);
    assert.deepEqual(input, { command: "pwd" });
    assert.equal(warnings.length, 1);
    assert.match(warnings[0].message, /Symposium pre-tool-use hook failed/);
  }
});

test("cancelling a running hook returns without a warning", async (t) => {
  const root = temp(t);
  writeFileSync(join(root, "hook"), "setInterval(() => {}, 1000);");
  const controller = new AbortController();
  const { handlers, ctx, warnings } = setup(t, (event, payload) =>
    runHook(event, payload, root, process.execPath, controller.signal, 5000));
  ctx.signal = controller.signal;
  const pending = handlers.get("tool_call")({ toolCallId: "cancel-1", toolName: "bash", input: {} }, ctx);
  controller.abort();
  assert.equal(await pending, undefined);
  assert.deepEqual(warnings, []);
});

test("runHook reports command errors and malformed output", async (t) => {
  const root = temp(t);
  writeFileSync(join(root, "hook"), 'process.stderr.write("denied"); process.exit(2);');
  await assert.rejects(runHook("pre-tool-use", {}, root, process.execPath), /denied/);
  writeFileSync(join(root, "hook"), 'process.stdout.write("not JSON");');
  await assert.rejects(runHook("pre-tool-use", {}, root, process.execPath), /invalid hook output/);
  writeFileSync(join(root, "hook"), 'process.stdout.write("{}");');
  assert.deepEqual(await runHook("pre-tool-use", {}, root, process.execPath), {});
  writeFileSync(join(root, "hook"), '');
  assert.deepEqual(await runHook("stop", {}, root, process.execPath), {});
  await assert.rejects(runHook("pre-tool-use", {}, root, join(root, "missing")), /Symposium pre-tool-use/);
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(runHook("pre-tool-use", {}, root, process.execPath, controller.signal), /aborted/);
});
