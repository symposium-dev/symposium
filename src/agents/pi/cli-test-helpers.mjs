import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

let binary;

// Cargo metadata honors both CARGO_TARGET_DIR and .cargo/config.toml. These
// tests run after `cargo build --locked --bin cargo-agents` at the native target.
export function cargoAgentsBinary() {
  if (binary) return binary;
  if (process.env.SYMPOSIUM_TEST_BINARY) {
    binary = resolve(process.env.SYMPOSIUM_TEST_BINARY);
  } else {
    const metadata = spawnSync("cargo", [
      "metadata", "--locked", "--no-deps", "--format-version", "1",
      "--manifest-path", fileURLToPath(new URL("../../../Cargo.toml", import.meta.url)),
    ], {
      encoding: "utf8",
      timeout: 30_000,
    });
    assert.equal(metadata.status, 0, metadata.error?.message ?? metadata.stderr);
    const target = JSON.parse(metadata.stdout).target_directory;
    binary = join(target, "debug", process.platform === "win32" ? "cargo-agents.exe" : "cargo-agents");
  }
  assert.ok(existsSync(binary), `Missing ${binary}; run npm run test:integration to build the CLI first`);
  return binary;
}

export function temp(t) {
  const root = mkdtempSync(join(tmpdir(), "symposium-pi-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}
