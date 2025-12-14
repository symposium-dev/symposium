# Extension Packaging

This chapter documents how the VSCode extension is built and packaged for distribution.

## Overview

The extension packaging involves several steps:

1. **Build the Rust binary** (`symposium-acp-agent`) for the target platform(s)
2. **Build the TypeScript/webpack bundle** (extension code + webview)
3. **Package as `.vsix`** using `vsce`

## Directory Structure

```
vscode-extension/
├── bin/                          # Bundled binaries (gitignored)
│   └── darwin-arm64/             # Platform-specific directories
│       └── symposium-acp-agent   # The conductor binary
├── out/                          # Compiled JS output (gitignored)
│   ├── extension.js              # Main extension bundle
│   └── webview.js                # Webview bundle
├── src/                          # TypeScript source
├── vendor/                       # -> ../vendor (symlink or path)
├── package.json
├── webpack.config.js
├── .vscodeignore                 # Files to exclude from .vsix
└── symposium-0.0.1.vsix          # Packaged extension (gitignored)
```

## Build Steps

### 1. Build the Rust Binary

The `symposium-acp-agent` binary must be compiled for each target platform and placed in `bin/<platform>-<arch>/`:

```bash
# For local development (current platform only)
cargo build --release -p symposium-acp-agent

# Copy to the expected location
mkdir -p vscode-extension/bin/darwin-arm64
cp target/release/symposium-acp-agent vscode-extension/bin/darwin-arm64/
```

Platform directory names follow Node.js conventions:
- `darwin-arm64` (macOS Apple Silicon)
- `darwin-x64` (macOS Intel)
- `linux-x64` (Linux x86_64)
- `win32-x64` (Windows x86_64)

### 2. Build the Vendored mynah-ui

The extension uses a vendored fork of mynah-ui. It must be built before the extension:

```bash
cd vendor/mynah-ui
npm ci
npm run build
```

### 3. Build the Extension

The extension uses webpack to bundle the TypeScript code:

```bash
cd vscode-extension
npm ci
npm run webpack  # Production build
```

This produces two bundles:
- `out/extension.js` - The main extension (Node.js target)
- `out/webview.js` - The webview code (browser target)

### 4. Package as .vsix

Use `vsce` to create the installable package:

```bash
cd vscode-extension
npx vsce package
```

This creates `symposium-<version>.vsix`.

## Binary Resolution at Runtime

The extension looks for the conductor binary in this order (see `binaryPath.ts`):

1. **Bundled binary**: `<extensionPath>/bin/<platform>-<arch>/symposium-acp-agent`
2. **Simple layout**: `<extensionPath>/bin/symposium-acp-agent` (for single-platform dev)
3. **PATH fallback**: Just `symposium-acp-agent` (development mode)

This allows development without bundling binaries - just `cargo install` the binary and it will be found in PATH.

## .vscodeignore

The `.vscodeignore` file controls what goes into the `.vsix`:

```
.vscode/**
.vscode-test/**
src/**
.gitignore
tsconfig.json
**/*.map
**/*.ts
```

Currently missing entries that should be added:
- `../vendor/**` - The vendored mynah-ui source (only the built webview.js is needed)
- `node_modules/**` - Should be excluded since webpack bundles dependencies

## Multi-Platform Distribution

The extension uses **platform-specific packages**. VSCode Marketplace natively supports this via the `--target` flag in `vsce`:

```bash
npx vsce package --target darwin-arm64
npx vsce package --target darwin-x64
npx vsce package --target linux-x64
npx vsce package --target linux-arm64
npx vsce package --target win32-x64
```

Each `.vsix` contains only that platform's binary (~7MB each). When users install from the marketplace, VSCode automatically downloads the correct platform variant.

### Target Platforms

| VSCode Target | Rust Target | Description |
|---------------|-------------|-------------|
| darwin-arm64 | aarch64-apple-darwin | macOS Apple Silicon |
| darwin-x64 | x86_64-apple-darwin | macOS Intel |
| linux-x64 | x86_64-unknown-linux-gnu | Linux x86_64 (glibc) |
| linux-arm64 | aarch64-unknown-linux-gnu | Linux ARM64 |
| win32-x64 | x86_64-pc-windows-msvc | Windows x86_64 |

We also build a musl variant (`x86_64-unknown-linux-musl`) for static linking, used in the standalone binary releases.

## CI/CD Release Workflow

Releases are automated via `.github/workflows/release-binaries.yml`, triggered when release-plz creates a `symposium-acp-agent-v*` tag:

1. **Build binaries** for all platforms (macOS, Linux, Windows, including ARM64 and musl)
2. **Upload to GitHub Release** as `.tar.gz` / `.zip` archives
3. **Build platform-specific VSCode extensions** using the binaries
4. **Upload `.vsix` files** to the GitHub Release

Future steps (TODO):
- Publish to VSCode Marketplace via `VSCE_PAT` secret
- Publish to Open VSX via `OVSX_PAT` secret

### CI Build Requirements

- macOS runner for darwin targets (Apple's codesigning requirements)
- Linux runner with cross-compilation tools for ARM64
- Linux runner with musl-tools for static builds
- Windows runner for MSVC builds

## Local Development

For local development without packaging:

```bash
# Install the conductor globally
cargo install --path src/symposium-acp-agent

# Build the extension
cd vscode-extension
npm ci
npm run compile  # or npm run watch

# Run in VSCode
# Press F5 to launch Extension Development Host
```

The extension will find `symposium-acp-agent` in PATH when no bundled binary exists.
