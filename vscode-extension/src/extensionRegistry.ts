/**
 * Extension Registry - Types and built-in extension definitions
 *
 * Extensions enrich agent capabilities through the Symposium proxy chain.
 * Uses the same distribution format as agents, with extensions stored
 * in the same registry JSON under an "extensions" array.
 */

import * as vscode from "vscode";
import { Distribution } from "./agentRegistry";

/**
 * Source tracking for extensions
 */
export type ExtensionSource = "built-in" | "registry" | "custom";

/**
 * Extension configuration - matches registry format
 */
export interface ExtensionConfig {
  id: string;
  name?: string;
  version?: string;
  description?: string;
  distribution: Distribution;
}

/**
 * Extension settings entry - stored in user settings
 * Built-ins only need id, registry needs _source, custom needs full distribution
 */
export interface ExtensionSettingsEntry {
  id: string;
  _enabled: boolean;
  _source: ExtensionSource;
  // Only present for custom extensions (registry fetches at runtime, built-in uses hardcoded)
  name?: string;
  description?: string;
  distribution?: Distribution;
}

/**
 * Registry entry format for extensions
 */
export interface ExtensionRegistryEntry {
  id: string;
  name: string;
  version: string;
  description?: string;
  distribution: Distribution;
}

/**
 * Built-in extensions - always available, use symposium distribution
 */
export const BUILT_IN_EXTENSIONS: ExtensionConfig[] = [
  {
    id: "sparkle",
    name: "Sparkle",
    description: "AI collaboration identity and embodiment",
    distribution: { symposium: { subcommand: "sparkle" } },
  },
  {
    id: "ferris",
    name: "Ferris",
    description: "Rust development tools (crate sources)",
    distribution: { symposium: { subcommand: "ferris" } },
  },
  {
    id: "cargo",
    name: "Cargo",
    description: "Cargo build and run tools",
    distribution: { symposium: { subcommand: "cargo" } },
  },
];

/**
 * Built-in extension IDs for quick lookup
 */
export const BUILT_IN_EXTENSION_IDS = new Set(
  BUILT_IN_EXTENSIONS.map((e) => e.id),
);

/**
 * Default extensions configuration (all built-ins enabled)
 */
export const DEFAULT_EXTENSIONS: ExtensionSettingsEntry[] =
  BUILT_IN_EXTENSIONS.map((ext) => ({
    id: ext.id,
    _enabled: true,
    _source: "built-in" as const,
  }));

/**
 * Get extension metadata by ID (for built-ins)
 */
export function getBuiltInExtension(id: string): ExtensionConfig | undefined {
  return BUILT_IN_EXTENSIONS.find((e) => e.id === id);
}

/**
 * Old format for migration
 */
interface OldExtensionFormat {
  id: string;
  enabled?: boolean;
  _enabled?: boolean;
  _source?: ExtensionSource;
  name?: string;
  description?: string;
  distribution?: Distribution;
}

/**
 * Get extensions from user settings, migrating old format if needed
 */
export function getExtensionsFromSettings(): ExtensionSettingsEntry[] {
  const config = vscode.workspace.getConfiguration("symposium");
  const raw = config.get<OldExtensionFormat[]>("extensions");

  // If no setting exists, return defaults
  if (!raw || raw.length === 0) {
    return DEFAULT_EXTENSIONS;
  }

  // Migrate old format to new format
  return raw.map((ext): ExtensionSettingsEntry => {
    // Determine enabled state: prefer _enabled, fall back to enabled, default true
    const isEnabled = ext._enabled ?? ext.enabled ?? true;

    // Determine source
    let source: ExtensionSource = ext._source ?? "custom";
    if (!ext._source && BUILT_IN_EXTENSION_IDS.has(ext.id)) {
      source = "built-in";
    }

    // Build clean entry without old 'enabled' field
    const entry: ExtensionSettingsEntry = {
      id: ext.id,
      _enabled: isEnabled,
      _source: source,
    };

    // Copy optional fields for custom extensions
    if (ext.name) entry.name = ext.name;
    if (ext.description) entry.description = ext.description;
    if (ext.distribution) entry.distribution = ext.distribution;

    return entry;
  });
}

/**
 * Get display info for an extension (name, description)
 * Uses built-in metadata, registry cache, or settings entry
 */
export function getExtensionDisplayInfo(
  entry: ExtensionSettingsEntry,
  registryCache: ExtensionRegistryEntry[],
): { name: string; description: string } {
  // Check built-ins first
  const builtIn = getBuiltInExtension(entry.id);
  if (builtIn) {
    return {
      name: builtIn.name ?? entry.id,
      description: builtIn.description ?? "",
    };
  }

  // Check registry cache
  const registryEntry = registryCache.find((e) => e.id === entry.id);
  if (registryEntry) {
    return {
      name: registryEntry.name,
      description: registryEntry.description ?? "",
    };
  }

  // Fall back to settings entry
  return {
    name: entry.name ?? entry.id,
    description: entry.description ?? "",
  };
}

/**
 * Check if extensions match the default configuration
 */
function isDefaultExtensions(extensions: ExtensionSettingsEntry[]): boolean {
  if (extensions.length !== DEFAULT_EXTENSIONS.length) {
    return false;
  }

  for (let i = 0; i < extensions.length; i++) {
    const ext = extensions[i];
    const def = DEFAULT_EXTENSIONS[i];
    if (
      ext.id !== def.id ||
      ext._enabled !== def._enabled ||
      ext._source !== def._source
    ) {
      return false;
    }
    // Custom extensions have extra fields, so they're not default
    if (ext.distribution || ext.name || ext.description) {
      return false;
    }
  }

  return true;
}

/**
 * Save extensions to user settings
 * If extensions match defaults, removes the key from settings
 */
export async function saveExtensions(
  extensions: ExtensionSettingsEntry[],
): Promise<void> {
  const config = vscode.workspace.getConfiguration("symposium");

  // If it's the default config, remove the key entirely
  if (isDefaultExtensions(extensions)) {
    await config.update(
      "extensions",
      undefined,
      vscode.ConfigurationTarget.Global,
    );
  } else {
    await config.update(
      "extensions",
      extensions,
      vscode.ConfigurationTarget.Global,
    );
  }
}

/**
 * Add an extension to settings
 */
export async function addExtension(
  entry: ExtensionSettingsEntry,
): Promise<void> {
  const current = getExtensionsFromSettings();

  // Don't add duplicates
  if (current.some((e) => e.id === entry.id)) {
    return;
  }

  await saveExtensions([...current, entry]);
}

/**
 * Remove an extension from settings
 */
export async function removeExtension(id: string): Promise<void> {
  const current = getExtensionsFromSettings();
  await saveExtensions(current.filter((e) => e.id !== id));
}

/**
 * Toggle extension enabled state
 */
export async function toggleExtension(id: string): Promise<void> {
  const current = getExtensionsFromSettings();
  const updated = current.map((e) =>
    e.id === id ? { ...e, _enabled: !e._enabled } : e,
  );
  await saveExtensions(updated);
}

/**
 * Reorder extensions
 */
export async function reorderExtensions(
  newOrder: Array<{ id: string; _enabled: boolean }>,
): Promise<void> {
  const current = getExtensionsFromSettings();

  // Preserve full entries, just reorder
  const byId = new Map(current.map((e) => [e.id, e]));
  const reordered = newOrder
    .map((item) => {
      const entry = byId.get(item.id);
      if (entry) {
        return { ...entry, _enabled: item._enabled };
      }
      return null;
    })
    .filter((e): e is ExtensionSettingsEntry => e !== null);

  await saveExtensions(reordered);
}

/**
 * Get enabled extensions in order (for passing to symposium)
 */
export function getEnabledExtensionIds(): string[] {
  return getExtensionsFromSettings()
    .filter((e) => e._enabled)
    .map((e) => e.id);
}

/**
 * Rewrite GitHub URLs to raw content URLs
 */
function rewriteGitHubUrl(url: string): string {
  // Convert github.com URLs to raw.githubusercontent.com
  // e.g., https://github.com/user/repo/blob/main/extension.json
  //    -> https://raw.githubusercontent.com/user/repo/main/extension.json
  const githubMatch = url.match(
    /^https:\/\/github\.com\/([^/]+)\/([^/]+)\/blob\/(.+)$/,
  );
  if (githubMatch) {
    const [, owner, repo, path] = githubMatch;
    return `https://raw.githubusercontent.com/${owner}/${repo}/${path}`;
  }
  return url;
}

/**
 * Fetch extension.json from a URL
 */
async function fetchExtensionFromUrl(
  url: string,
): Promise<ExtensionRegistryEntry> {
  const rawUrl = rewriteGitHubUrl(url);
  const response = await fetch(rawUrl);
  if (!response.ok) {
    throw new Error(
      `Failed to fetch: ${response.status} ${response.statusText}`,
    );
  }
  const data = (await response.json()) as ExtensionRegistryEntry;
  if (!data.id || !data.distribution) {
    throw new Error("Invalid extension.json: missing id or distribution");
  }
  return data;
}

/**
 * Custom extension types for the QuickPick
 */
type CustomExtensionType = "executable" | "npx" | "pipx" | "url";

/**
 * QuickPick item with extension data
 */
interface ExtensionQuickPickItem extends vscode.QuickPickItem {
  extensionId?: string;
  source?: ExtensionSource;
  customType?: CustomExtensionType;
  isSeparator?: boolean;
}

/**
 * Show the Add Extension dialog
 * Returns true if an extension was added
 */
export async function showAddExtensionDialog(
  registryExtensions: ExtensionRegistryEntry[],
): Promise<boolean> {
  const currentExtensions = getExtensionsFromSettings();
  const currentIds = new Set(currentExtensions.map((e) => e.id));

  // Build QuickPick items with sections
  const items: ExtensionQuickPickItem[] = [];

  // Built-in section
  items.push({
    label: "Built-in",
    kind: vscode.QuickPickItemKind.Separator,
  });

  for (const ext of BUILT_IN_EXTENSIONS) {
    const alreadyAdded = currentIds.has(ext.id);
    items.push({
      label: alreadyAdded ? `$(check) ${ext.name}` : (ext.name ?? ext.id),
      description: alreadyAdded ? "(already added)" : undefined,
      detail: ext.description,
      extensionId: ext.id,
      source: "built-in",
      // Disabled items can still be selected but we'll filter them
    });
  }

  // Registry section (if any)
  if (registryExtensions.length > 0) {
    items.push({
      label: "From Registry",
      kind: vscode.QuickPickItemKind.Separator,
    });

    for (const ext of registryExtensions) {
      const alreadyAdded = currentIds.has(ext.id);
      items.push({
        label: alreadyAdded ? `$(check) ${ext.name}` : ext.name,
        description: alreadyAdded ? "(already added)" : `v${ext.version}`,
        detail: ext.description,
        extensionId: ext.id,
        source: "registry",
      });
    }
  }

  // Custom section
  items.push({
    label: "Add Custom Extension",
    kind: vscode.QuickPickItemKind.Separator,
  });

  items.push({
    label: "$(terminal) From executable on your system",
    detail: "Specify a local command or path to run",
    customType: "executable",
  });

  items.push({
    label: "$(package) From npx package",
    detail: "Run an npm package via npx",
    customType: "npx",
  });

  items.push({
    label: "$(snake) From pipx package",
    detail: "Run a Python package via pipx",
    customType: "pipx",
  });

  items.push({
    label: "$(link) From URL to extension.json",
    detail:
      "Fetch extension definition from a URL (GitHub URLs auto-converted)",
    customType: "url",
  });

  // Show QuickPick
  const selected = await vscode.window.showQuickPick(items, {
    placeHolder: "Select an extension to add",
    title: "Add Extension",
    matchOnDescription: true,
    matchOnDetail: true,
  });

  if (!selected) {
    return false;
  }

  // Handle built-in or registry selection
  if (selected.extensionId && selected.source) {
    if (currentIds.has(selected.extensionId)) {
      vscode.window.showInformationMessage(
        `${selected.label.replace("$(check) ", "")} is already added.`,
      );
      return false;
    }

    const entry: ExtensionSettingsEntry = {
      id: selected.extensionId,
      _enabled: true,
      _source: selected.source,
    };
    await addExtension(entry);
    vscode.window.showInformationMessage(`Added extension: ${selected.label}`);
    return true;
  }

  // Handle custom extension
  if (selected.customType) {
    return handleCustomExtension(selected.customType);
  }

  return false;
}

/**
 * Handle adding a custom extension
 */
async function handleCustomExtension(
  type: CustomExtensionType,
): Promise<boolean> {
  switch (type) {
    case "executable": {
      const command = await vscode.window.showInputBox({
        prompt: "Enter the command or path to the executable",
        placeHolder: "/usr/local/bin/my-extension or my-extension",
        title: "Add Custom Extension - Executable",
      });
      if (!command) {
        return false;
      }

      const id = await vscode.window.showInputBox({
        prompt: "Enter a unique ID for this extension",
        placeHolder: "my-extension",
        title: "Add Custom Extension - ID",
        value:
          command
            .split("/")
            .pop()
            ?.replace(/\.[^.]+$/, "") ?? "custom",
      });
      if (!id) {
        return false;
      }

      const entry: ExtensionSettingsEntry = {
        id,
        _enabled: true,
        _source: "custom",
        name: id,
        distribution: {
          local: { command },
        },
      };
      await addExtension(entry);
      vscode.window.showInformationMessage(`Added custom extension: ${id}`);
      return true;
    }

    case "npx": {
      const packageName = await vscode.window.showInputBox({
        prompt: "Enter the npm package name",
        placeHolder: "@myorg/my-extension or my-extension",
        title: "Add Custom Extension - npx Package",
      });
      if (!packageName) {
        return false;
      }

      // Derive ID from package name
      const id = packageName.replace(/^@[^/]+\//, "").replace(/@.*$/, "");

      const entry: ExtensionSettingsEntry = {
        id,
        _enabled: true,
        _source: "custom",
        name: packageName,
        distribution: {
          npx: { package: packageName },
        },
      };
      await addExtension(entry);
      vscode.window.showInformationMessage(
        `Added custom extension: ${packageName}`,
      );
      return true;
    }

    case "pipx": {
      const packageName = await vscode.window.showInputBox({
        prompt: "Enter the Python package name",
        placeHolder: "my-extension",
        title: "Add Custom Extension - pipx Package",
      });
      if (!packageName) {
        return false;
      }

      const entry: ExtensionSettingsEntry = {
        id: packageName,
        _enabled: true,
        _source: "custom",
        name: packageName,
        distribution: {
          pipx: { package: packageName },
        },
      };
      await addExtension(entry);
      vscode.window.showInformationMessage(
        `Added custom extension: ${packageName}`,
      );
      return true;
    }

    case "url": {
      const url = await vscode.window.showInputBox({
        prompt: "Enter URL to extension.json",
        placeHolder: "https://github.com/user/repo/blob/main/extension.json",
        title: "Add Custom Extension - URL",
      });
      if (!url) {
        return false;
      }

      try {
        const extensionData = await vscode.window.withProgress(
          {
            location: vscode.ProgressLocation.Notification,
            title: "Fetching extension.json...",
          },
          () => fetchExtensionFromUrl(url),
        );

        const entry: ExtensionSettingsEntry = {
          id: extensionData.id,
          _enabled: true,
          _source: "custom",
          name: extensionData.name,
          description: extensionData.description,
          distribution: extensionData.distribution,
        };
        await addExtension(entry);
        vscode.window.showInformationMessage(
          `Added extension: ${extensionData.name}`,
        );
        return true;
      } catch (error) {
        vscode.window.showErrorMessage(
          `Failed to fetch extension: ${error instanceof Error ? error.message : String(error)}`,
        );
        return false;
      }
    }

    default:
      return false;
  }
}
