/**
 * Agent Registry - Types and built-in agent definitions
 *
 * Supports multiple distribution methods (npx, pipx, binary) and
 * merges built-in agents with user-configured agents from settings.
 *
 * Registry fetching and distribution resolution are delegated to the
 * symposium-acp-agent binary via `registry list` and `registry resolve`
 * subcommands.
 */

import * as vscode from "vscode";
import * as os from "os";
import * as path from "path";
import * as fs from "fs";
import { promisify } from "util";
import { exec, spawn } from "child_process";
import { getConductorCommand } from "./binaryPath";
import { logger } from "./extension";

const execAsync = promisify(exec);

/**
 * Extension context - must be set via setExtensionContext before using registry functions
 */
let extensionContext: vscode.ExtensionContext | undefined;

/**
 * Set the extension context for binary path resolution
 */
export function setExtensionContext(context: vscode.ExtensionContext): void {
  extensionContext = context;
}

/**
 * Run a symposium-acp-agent registry subcommand and return stdout.
 */
export async function runRegistryCommand(args: string[]): Promise<string> {
  if (!extensionContext) {
    throw new Error(
      "Extension context not set - call setExtensionContext first",
    );
  }

  const command = getConductorCommand(extensionContext);

  return new Promise((resolve, reject) => {
    logger.important("agent", "Resolving extension", {
      args,
    });
    const proc = spawn(command, ["registry", ...args], {
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (data) => {
      stdout += data.toString();
    });

    proc.stderr.on("data", (data) => {
      stderr += data.toString();
    });

    proc.on("error", (err) => {
      reject(new Error(`Failed to spawn ${command}: ${err.message}`));
    });

    proc.on("close", (code) => {
      if (code === 0) {
        logger.important("agent", "Resolve success", {
          command,
          args,
          stdout,
        });
        resolve(stdout.trim());
      } else {
        logger.important("agent", "Resolve error", {
          command,
          args,
          code,
          stderr,
        });
        reject(
          new Error(
            `${command} registry ${args.join(" ")} failed with code ${code}: ${stderr}`,
          ),
        );
      }
    });
  });
}

/**
 * Availability status for built-in agents
 */
export interface AvailabilityStatus {
  available: boolean;
  reason?: string;
}

/**
 * Check if a command exists on the PATH
 */
async function commandExists(command: string): Promise<boolean> {
  try {
    const checkCmd =
      process.platform === "win32" ? `where ${command}` : `which ${command}`;
    await execAsync(checkCmd);
    return true;
  } catch {
    return false;
  }
}

/**
 * Check if a directory exists
 */
async function directoryExists(dirPath: string): Promise<boolean> {
  try {
    const stats = await fs.promises.stat(dirPath);
    return stats.isDirectory();
  } catch {
    return false;
  }
}

/**
 * Availability checks for built-in agents.
 * If an agent is not in this map, it's always available.
 */
/* eslint-disable @typescript-eslint/naming-convention -- agent IDs use kebab-case */
const AVAILABILITY_CHECKS: Record<string, () => Promise<AvailabilityStatus>> = {
  "zed-claude-code": async () => {
    const claudeDir = path.join(os.homedir(), ".claude");
    if (await directoryExists(claudeDir)) {
      return { available: true };
    }
    return { available: false, reason: "~/.claude not found" };
  },
  "kiro-cli": async () => {
    if (await commandExists("kiro-cli-chat")) {
      return { available: true };
    }
    return { available: false, reason: "kiro-cli-chat not found on PATH" };
  },
  // elizacp has no check - always available (symposium builtin)
};
/* eslint-enable @typescript-eslint/naming-convention */

/**
 * Check availability for a single agent
 */
export async function checkAgentAvailability(
  agentId: string,
): Promise<AvailabilityStatus> {
  const check = AVAILABILITY_CHECKS[agentId];
  if (!check) {
    return { available: true };
  }
  return check();
}

/**
 * Check availability for all built-in agents
 */
export async function checkAllBuiltInAvailability(): Promise<
  Map<string, AvailabilityStatus>
> {
  const results = new Map<string, AvailabilityStatus>();

  await Promise.all(
    BUILT_IN_AGENTS.map(async (agent) => {
      const status = await checkAgentAvailability(agent.id);
      results.set(agent.id, status);
    }),
  );

  return results;
}

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
    id: "elizacp",
    name: "ElizACP",
    description: "Built-in Eliza agent for testing",
    distribution: {
      symposium: { subcommand: "eliza" },
    },
    _source: "custom",
  },
  {
    id: "kiro-cli",
    name: "Kiro CLI",
    distribution: {
      local: { command: "kiro-cli-chat", args: ["acp"] },
    },
    _source: "custom",
  },
];

/**
 * Default agent ID when none is selected
 */
export const DEFAULT_AGENT_ID = "zed-claude-code";

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
 * Resolve an agent to a JSON string for passing to `symposium-acp-agent run-with --agent`.
 *
 * First tries `registry resolve-agent <id>` which handles:
 * - Built-in agents (elizacp, etc.)
 * - Registry agents (gemini, auggie, etc.)
 * - Binary downloads and caching
 *
 * Falls back to local distribution resolution for custom agents
 * configured in settings with explicit distribution.
 *
 * @throws Error if no compatible distribution is found
 */
export async function resolveAgentJson(agent: AgentConfig): Promise<string> {
  let config = {
    id: agent.id,
    name: agent.name,
    distribution: agent.distribution,
  };
  let resolved = await runRegistryCommand(["resolve-agent", JSON.stringify(config)]);
  // On the Rust side, returns an `McpServer`, which doesn't have an id
  return JSON.stringify({
    id: agent.id,
    ...(JSON.parse(resolved))
  });
}

/**
 * Registry entry format (as returned from `registry list` command)
 * Note: distribution is not included - use `registry resolve` to get spawn command
 */
export interface RegistryEntry {
  id: string;
  name: string;
  version?: string;
  description?: string;
}

/**
 * Cached registry list
 */
let registryCache: RegistryEntry[] | null = null;

/**
 * Fetch all agents from the registry via `symposium-acp-agent registry list`.
 * Includes both built-in agents and registry agents.
 */
export async function fetchRegistry(): Promise<RegistryEntry[]> {
  const output = await runRegistryCommand(["list"]);
  registryCache = JSON.parse(output) as RegistryEntry[];
  return registryCache;
}

/**
 * Get cached registry or fetch if not available
 */
export async function getRegistry(): Promise<RegistryEntry[]> {
  if (registryCache) {
    return registryCache;
  }
  return fetchRegistry();
}

/**
 * Clear the registry cache (call when refreshing)
 */
export function clearRegistryCache(): void {
  registryCache = null;
}

/**
 * Fetch agents from the registry that are NOT already in the user's effective agents list.
 */
export async function fetchAvailableRegistryAgents(): Promise<RegistryEntry[]> {
  const registry = await getRegistry();

  // Filter out agents already configured in settings
  const effectiveAgents = getEffectiveAgents();
  const existingIds = new Set(effectiveAgents.map((a) => a.id));

  return registry.filter((entry) => !existingIds.has(entry.id));
}

/**
 * Add an agent from the registry to user settings.
 * Only stores metadata - distribution is resolved at spawn time via `registry resolve`.
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
    // distribution not stored - resolved via `registry resolve` at spawn time
    distribution: {}, // empty distribution signals registry-sourced agent
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
        distribution: {}, // resolved via `registry resolve` at spawn time
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
