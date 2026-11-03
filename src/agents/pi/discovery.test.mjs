import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";
import { DefaultPackageManager, SettingsManager } from "@earendil-works/pi-coding-agent";
import { cargoAgentsBinary, temp } from "./cli-test-helpers.mjs";

// This reader is not publicly exported. Resolve it relative to the pinned Pi
// package, so these tests exercise Pi's actual project-file discovery rules.
const { loadMcpConfig } = await import(new URL(
  "./extensions/mcp/config.js", import.meta.resolve("@earendil-works/pi-coding-agent"),
));

function syncedWorkspace(t, syncFrom = "member") {
  const root = temp(t);
  const home = join(root, "home");
  const workspace = join(root, "workspace");
  const member = join(workspace, "member");
  const config = join(root, "symposium");
  const agentDir = join(home, ".pi", "agent");
  const plugin = join(config, "plugins", "discovery-probe");
  const skill = join(plugin, "skills", "member-guide");
  for (const dir of [home, agentDir, join(member, "src"), join(workspace, ".git"), skill]) {
    mkdirSync(dir, { recursive: true });
  }
  writeFileSync(join(workspace, "Cargo.toml"), '[workspace]\nmembers = ["member"]\nresolver = "2"\n');
  writeFileSync(join(member, "Cargo.toml"), '[package]\nname = "member"\nversion = "0.1.0"\nedition = "2021"\n');
  writeFileSync(join(member, "src", "lib.rs"), "");
  writeFileSync(join(config, "config.toml"), `
auto-update = "off"
auto-sync = false
hook-scope = "project"
[defaults]
symposium-recommendations = false
user-plugins = true
[[agent]]
name = "pi"
`);
  writeFileSync(join(plugin, "Symposium.toml"), `
name = "discovery-probe"
depends-on = ["*"]
[[skills]]
source.path = "skills"
[[mcp_servers]]
name = "member-probe"
command = ${JSON.stringify(process.execPath.replaceAll("\\", "/"))}
args = ["--version"]
env = []
`);
  writeFileSync(join(skill, "SKILL.md"), "---\nname: member-guide\ndescription: Workspace member guidance\n---\nUse the local fixture.\n");

  const env = {
    ...process.env,
    HOME: home,
    USERPROFILE: home,
    // Keep the installed toolchain available after isolating the child home.
    CARGO_HOME: process.env.CARGO_HOME || join(homedir(), ".cargo"),
    RUSTUP_HOME: process.env.RUSTUP_HOME || join(homedir(), ".rustup"),
    CARGO_NET_OFFLINE: "true",
    SYMPOSIUM_HOME: config,
    PI_CODING_AGENT_DIR: agentDir,
  };
  delete env.SYMPOSIUM_CARGO;
  const result = spawnSync(cargoAgentsBinary(), ["sync"], {
    cwd: syncFrom === "member" ? member : workspace, env, encoding: "utf8", timeout: 20_000,
  });
  assert.equal(result.error, undefined, `sync must execute: ${result.error}`);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.ok([workspace, member].some(dir =>
    existsSync(join(dir, ".agents", "skills", "member-guide", ".symposium"))),
    "the real sync must install the fixture's managed skill");
  return { workspace, member, agentDir };
}

async function extensions(cwd, agentDir) {
  const manager = new DefaultPackageManager({ cwd, agentDir, settingsManager: SettingsManager.inMemory() });
  const resources = await manager.resolve();
  return resources.extensions.filter(entry => basename(entry.path) === "symposium.ts");
}

function servers(cwd, agentDir) {
  const config = loadMcpConfig({ cwd, agentDir, projectTrusted: true });
  assert.deepEqual(config.errors, [], "the generated MCP configuration must be valid for Pi");
  return config.servers.map(server => server.name);
}

test("Pi discovers synced project integration from the workspace root", async t => {
  const { workspace, agentDir } = syncedWorkspace(t, "workspace");
  assert.equal((await extensions(workspace, agentDir)).length, 1);
  assert.deepEqual(servers(workspace, agentDir), ["member-probe"]);
});

test("Pi discovers the synced project extension from a workspace member", async t => {
  const { member, agentDir } = syncedWorkspace(t);
  assert.equal((await extensions(member, agentDir)).length, 1,
    "a member session must load Symposium's bridge for hooks and generated skill discovery");
});

test("Pi discovers synced project MCP servers from a workspace member", t => {
  const { member, agentDir } = syncedWorkspace(t);
  assert.deepEqual(servers(member, agentDir), ["member-probe"],
    "a member session must see the MCP server installed by sync in that directory");
});
