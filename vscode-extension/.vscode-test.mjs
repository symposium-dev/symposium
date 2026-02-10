import { defineConfig } from "@vscode/test-cli";

export default defineConfig({
  files: "out/test/**/*.test.js",
  // Pin to specific version for reproducible tests and better caching
  version: "1.108.2",
  workspaceFolder: "./test-workspace",
  launchArgs: [
    "--user-data-dir=/tmp/symposium-vscode-test-user",
    "--extensions-dir=/tmp/symposium-vscode-test-extensions",
  ],
  mocha: {
    ui: "tdd",
    timeout: 20000,
  },
});
