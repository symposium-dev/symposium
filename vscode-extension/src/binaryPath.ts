/**
 * Binary path resolution for bundled symposium-acp-agent
 */

import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";

/**
 * Get the path to the bundled symposium-acp-agent binary.
 * Returns undefined if the binary is not found (e.g., development mode without bundled binary).
 */
export function getBundledBinaryPath(
  context: vscode.ExtensionContext,
): string | undefined {
  const platform = process.platform;
  const arch = process.arch;

  // Map Node.js platform/arch to our binary naming
  let binaryName: string;
  if (platform === "win32") {
    binaryName = "symposium-acp-agent.exe";
  } else {
    binaryName = "symposium-acp-agent";
  }

  // Binary is stored in bin/<platform>-<arch>/
  const platformDir = `${platform}-${arch}`;
  const binaryPath = path.join(
    context.extensionPath,
    "bin",
    platformDir,
    binaryName,
  );

  if (fs.existsSync(binaryPath)) {
    return binaryPath;
  }

  // Also check for binary directly in bin/ (simpler layout for single-platform dev)
  const simplePath = path.join(context.extensionPath, "bin", binaryName);
  if (fs.existsSync(simplePath)) {
    return simplePath;
  }

  return undefined;
}

/**
 * Get the conductor command - either bundled binary or fall back to PATH
 */
export function getConductorCommand(
  context: vscode.ExtensionContext,
): string {
  const bundledPath = getBundledBinaryPath(context);
  if (bundledPath) {
    return bundledPath;
  }

  // Fall back to expecting it in PATH (development mode)
  return "symposium-acp-agent";
}
