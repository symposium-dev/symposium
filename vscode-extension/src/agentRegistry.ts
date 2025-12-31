/**
 * Agent Registry - Types and built-in agent definitions
 *
 * Supports multiple distribution methods (npx, pipx, binary) and
 * merges built-in agents with user-configured agents from settings.
 */

const REGISTRY_URL =
  "https://github.com/agentclientprotocol/registry/releases/latest/download/registry.json";

import * as vscode from "vscode";
import * as os from "os";
import * as path from "path";

/**
 * Distribution methods for spawning an agent
 */
export interface NpxDistribution {
  package: string;
  args?: string[];
}

export interface PipxDistribution {
  package: string;
  args?: string[];
}

export interface BinaryDistribution {
  archive: string;
  cmd: string;
  args?: string[];
}

export interface SymposiumDistribution {
  subcommand: string;
  args?: string[];
}

export interface LocalDistribution {
  command: string;
  args?: string[];
  env?: Record<string, string>;
}

export interface Distribution {
  local?: LocalDistribution; // explicit local binary path
  npx?: NpxDistribution;
  pipx?: PipxDistribution;
  binary?: Record<string, BinaryDistribution>; // keyed by platform, e.g., "darwin-aarch64"
  symposium?: SymposiumDistribution; // built-in to symposium-acp-agent
}

/**
 * Agent configuration - matches registry format
 */
export interface AgentConfig {
  id: string;
  distribution: Distribution;
  name?: string;
  version?: string;
  description?: string;
  _source?: "registry" | "custom";
}

/**
 * Settings format - object keyed by agent id (id is implicit in key)
 */
export type AgentSettingsEntry = Omit<AgentConfig, "id">;
export type AgentSettings = Record<string, AgentSettingsEntry>;

/**
 * Built-in agents - these are always available unless overridden in settings
 */
export const BUILT_IN_AGENTS: AgentConfig[] = [
  {
    id: "zed-claude-code",
    name: "Claude Code",
    distribution: {
      npx: { package: "@zed-industries/claude-code-acp@latest" },
    },
    _source: "custom",
  },
  {
    id: "zed-codex",
    name: "Codex",
    distribution: {
      npx: { package: "@zed-industries/codex-acp@latest" },
    },
    _source: "custom",
  },
  {
    id: "google-gemini",
    name: "Gemini",
    distribution: {
      npx: {
        package: "@google/gemini-cli@latest",
        args: ["--experimental-acp"],
      },
    },
    _source: "custom",
  },
  {
    id: "elizacp",
    name: "ElizACP",
    description: "Built-in Eliza agent for testing",
    distribution: {
      symposium: { subcommand: "eliza" },
    },
    _source: "custom",
  },
];

/**
 * Default agent ID when none is selected
 */
export const DEFAULT_AGENT_ID = "zed-claude-code";

/**
 * Get the current platform key for binary distribution lookup
 */
export function getPlatformKey(): string {
  const platform = process.platform;
  const arch = process.arch;

  const platformMap: Record<string, Record<string, string>> = {
    darwin: {
      arm64: "darwin-aarch64",
      x64: "darwin-x86_64",
    },
    linux: {
      x64: "linux-x86_64",
      arm64: "linux-aarch64",
    },
    win32: {
      x64: "windows-x86_64",
    },
  };

  return platformMap[platform]?.[arch] ?? `${platform}-${arch}`;
}

/**
 * Get the cache directory for binary agents
 */
export function getBinaryCacheDir(agentId: string, version: string): string {
  return path.join(os.homedir(), ".symposium", "bin", agentId, version);
}

/**
 * Merge built-in agents with user settings.
 * Settings entries override built-ins with the same id.
 */
export function getEffectiveAgents(): AgentConfig[] {
  const config = vscode.workspace.getConfiguration("symposium");
  const settingsAgents = config.get<AgentSettings>("agents", {});

  // Start with built-ins
  const agentsById = new Map<string, AgentConfig>();
  for (const agent of BUILT_IN_AGENTS) {
    agentsById.set(agent.id, agent);
  }

  // Override/add from settings
  for (const [id, entry] of Object.entries(settingsAgents)) {
    agentsById.set(id, { id, ...entry });
  }

  return Array.from(agentsById.values());
}

/**
 * Get a specific agent by ID
 */
export function getAgentById(id: string): AgentConfig | undefined {
  const agents = getEffectiveAgents();
  return agents.find((a) => a.id === id);
}

/**
 * Get the currently selected agent ID from settings
 */
export function getCurrentAgentId(): string {
  const config = vscode.workspace.getConfiguration("symposium");
  return config.get<string>("currentAgentId", DEFAULT_AGENT_ID);
}

/**
 * Get the currently selected agent config
 */
export function getCurrentAgent(): AgentConfig | undefined {
  return getAgentById(getCurrentAgentId());
}

/**
 * Resolved spawn command
 */
export interface ResolvedCommand {
  command: string;
  args: string[];
  env?: Record<string, string>;
  /** If true, this is a built-in symposium subcommand - don't wrap with conductor */
  isSymposiumBuiltin?: boolean;
}

/**
 * Resolve an agent's distribution to a spawn command.
 * Priority: symposium > npx > pipx > binary
 *
 * @throws Error if no compatible distribution is found
 */
export async function resolveDistribution(
  agent: AgentConfig,
): Promise<ResolvedCommand> {
  const dist = agent.distribution;

  // Try local first (explicit path takes priority)
  if (dist.local) {
    return {
      command: dist.local.command,
      args: dist.local.args ?? [],
      env: dist.local.env,
    };
  }

  // Try symposium builtin (e.g., eliza subcommand)
  if (dist.symposium) {
    return {
      command: dist.symposium.subcommand,
      args: dist.symposium.args ?? [],
      isSymposiumBuiltin: true,
    };
  }

  // Try npx
  if (dist.npx) {
    return {
      command: "npx",
      args: ["-y", dist.npx.package, ...(dist.npx.args ?? [])],
    };
  }

  // Try pipx
  if (dist.pipx) {
    return {
      command: "pipx",
      args: ["run", dist.pipx.package, ...(dist.pipx.args ?? [])],
    };
  }

  // Try binary for current platform
  if (dist.binary) {
    const platformKey = getPlatformKey();
    const binaryDist = dist.binary[platformKey];

    if (binaryDist) {
      const version = agent.version ?? "latest";
      const cacheDir = getBinaryCacheDir(agent.id, version);
      // cmd may have leading "./" - strip it for the path
      const executable = binaryDist.cmd.replace(/^\.\//, "");
      const executablePath = path.join(cacheDir, executable);

      // Check if binary exists in cache
      const fs = await import("fs/promises");
      try {
        await fs.access(executablePath);
      } catch {
        // Binary not cached - need to download
        await downloadAndCacheBinary(agent, binaryDist, cacheDir);
      }

      return {
        command: executablePath,
        args: binaryDist.args ?? [],
      };
    }
  }

  throw new Error(
    `No compatible distribution found for agent "${agent.id}" on platform ${getPlatformKey()}`,
  );
}

/**
 * Download and cache a binary distribution
 */
async function downloadAndCacheBinary(
  agent: AgentConfig,
  binaryDist: BinaryDistribution,
  cacheDir: string,
): Promise<void> {
  const fs = await import("fs/promises");

  // Clean up old versions first
  const parentDir = path.dirname(cacheDir);
  try {
    const entries = await fs.readdir(parentDir);
    for (const entry of entries) {
      const entryPath = path.join(parentDir, entry);
      if (entryPath !== cacheDir) {
        await fs.rm(entryPath, { recursive: true, force: true });
      }
    }
  } catch {
    // Parent directory doesn't exist yet, that's fine
  }

  // Create cache directory
  await fs.mkdir(cacheDir, { recursive: true });

  // Download the binary
  const response = await fetch(binaryDist.archive);
  if (!response.ok) {
    throw new Error(
      `Failed to download binary for ${agent.id}: ${response.status} ${response.statusText}`,
    );
  }

  const buffer = await response.arrayBuffer();
  const url = new URL(binaryDist.archive);
  const filename = path.basename(url.pathname);
  const downloadPath = path.join(cacheDir, filename);

  await fs.writeFile(downloadPath, Buffer.from(buffer));

  // Extract if it's an archive
  if (
    filename.endsWith(".tar.gz") ||
    filename.endsWith(".tgz") ||
    filename.endsWith(".zip")
  ) {
    await extractArchive(downloadPath, cacheDir);
    // Remove the archive after extraction
    await fs.unlink(downloadPath);
  }

  // Make executable on Unix
  if (process.platform !== "win32") {
    const executable = binaryDist.cmd.replace(/^\.\//, "");
    const executablePath = path.join(cacheDir, executable);
    await fs.chmod(executablePath, 0o755);
  }
}

/**
 * Extract an archive to a directory
 */
async function extractArchive(
  archivePath: string,
  destDir: string,
): Promise<void> {
  const { exec } = await import("child_process");
  const { promisify } = await import("util");
  const execAsync = promisify(exec);

  if (archivePath.endsWith(".zip")) {
    if (process.platform === "win32") {
      await execAsync(
        `powershell -command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}'"`,
      );
    } else {
      await execAsync(`unzip -o "${archivePath}" -d "${destDir}"`);
    }
  } else {
    // tar.gz or tgz
    await execAsync(`tar -xzf "${archivePath}" -C "${destDir}"`);
  }
}

/**
 * Registry entry format (as returned from the registry API)
 */
export interface RegistryEntry {
  id: string;
  name: string;
  version: string;
  description?: string;
  distribution: Distribution;
}

/**
 * Registry JSON format
 */
interface RegistryJson {
  version: string;
  agents: RegistryEntry[];
}

/**
 * Fetch the agent registry from GitHub releases.
 * Returns agents that are NOT already in the user's effective agents list.
 */
export async function fetchAvailableRegistryAgents(): Promise<RegistryEntry[]> {
  const response = await fetch(REGISTRY_URL);
  if (!response.ok) {
    throw new Error(
      `Failed to fetch registry: ${response.status} ${response.statusText}`,
    );
  }

  const registryJson = (await response.json()) as RegistryJson;

  // Filter out agents already configured
  const effectiveAgents = getEffectiveAgents();
  const existingIds = new Set(effectiveAgents.map((a) => a.id));

  return registryJson.agents.filter((entry) => !existingIds.has(entry.id));
}

/**
 * Fetch all agents from the registry (without filtering)
 */
export async function fetchRegistry(): Promise<RegistryEntry[]> {
  const response = await fetch(REGISTRY_URL);
  if (!response.ok) {
    throw new Error(
      `Failed to fetch registry: ${response.status} ${response.statusText}`,
    );
  }

  const registryJson = (await response.json()) as RegistryJson;
  return registryJson.agents;
}

/**
 * Add an agent from the registry to user settings
 */
export async function addAgentFromRegistry(
  entry: RegistryEntry,
): Promise<void> {
  const config = vscode.workspace.getConfiguration("symposium");
  const currentAgents = config.get<AgentSettings>("agents", {});

  const newEntry: AgentSettingsEntry = {
    name: entry.name,
    version: entry.version,
    description: entry.description,
    distribution: entry.distribution,
    _source: "registry",
  };

  const updatedAgents = {
    ...currentAgents,
    [entry.id]: newEntry,
  };

  await config.update(
    "agents",
    updatedAgents,
    vscode.ConfigurationTarget.Global,
  );
}

/**
 * Check for updates to registry-sourced agents and update them in settings.
 * Returns a summary of what was updated.
 */
export async function checkForRegistryUpdates(): Promise<{
  updated: string[];
  failed: string[];
}> {
  const result = { updated: [] as string[], failed: [] as string[] };

  // Fetch the registry
  let registryAgents: RegistryEntry[];
  try {
    registryAgents = await fetchRegistry();
  } catch (error) {
    throw new Error(
      `Failed to fetch registry: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  // Create a lookup map by agent id
  const registryById = new Map<string, RegistryEntry>();
  for (const agent of registryAgents) {
    registryById.set(agent.id, agent);
  }

  // Get current settings
  const config = vscode.workspace.getConfiguration("symposium");
  const currentAgents = config.get<AgentSettings>("agents", {});

  // Find registry-sourced agents that have updates
  const updates: Record<string, AgentSettingsEntry> = {};

  for (const [id, entry] of Object.entries(currentAgents)) {
    if (entry._source !== "registry") {
      continue;
    }

    const registryEntry = registryById.get(id);
    if (!registryEntry) {
      // Agent was removed from registry - leave as-is
      continue;
    }

    // Check if version changed
    if (entry.version !== registryEntry.version) {
      updates[id] = {
        name: registryEntry.name,
        version: registryEntry.version,
        description: registryEntry.description,
        distribution: registryEntry.distribution,
        _source: "registry",
      };
      result.updated.push(registryEntry.name);
    }
  }

  // Apply updates if any
  if (Object.keys(updates).length > 0) {
    const updatedAgents = {
      ...currentAgents,
      ...updates,
    };

    await config.update(
      "agents",
      updatedAgents,
      vscode.ConfigurationTarget.Global,
    );
  }

  return result;
}

/**
 * Show a QuickPick dialog to add an agent from the registry
 */
export async function showAddAgentFromRegistryDialog(): Promise<boolean> {
  // Show progress while fetching
  const agents = await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Fetching agent registry...",
      cancellable: false,
    },
    async () => {
      try {
        return await fetchAvailableRegistryAgents();
      } catch (error) {
        vscode.window.showErrorMessage(
          `Failed to fetch registry: ${error instanceof Error ? error.message : String(error)}`,
        );
        return null;
      }
    },
  );

  if (agents === null) {
    return false;
  }

  if (agents.length === 0) {
    vscode.window.showInformationMessage(
      "All registry agents are already configured.",
    );
    return false;
  }

  // Create QuickPick items
  interface AgentQuickPickItem extends vscode.QuickPickItem {
    agent: RegistryEntry;
  }

  const items: AgentQuickPickItem[] = agents.map((agent) => ({
    label: agent.name,
    description: `v${agent.version}`,
    detail: agent.description,
    agent,
  }));

  const selected = await vscode.window.showQuickPick(items, {
    placeHolder: "Select an agent to add",
    title: "Add Agent from Registry",
    matchOnDescription: true,
    matchOnDetail: true,
  });

  if (!selected) {
    return false;
  }

  try {
    await addAgentFromRegistry(selected.agent);
    vscode.window.showInformationMessage(
      `Added ${selected.agent.name} to your agents.`,
    );
    return true;
  } catch (error) {
    vscode.window.showErrorMessage(
      `Failed to add agent: ${error instanceof Error ? error.message : String(error)}`,
    );
    return false;
  }
}
