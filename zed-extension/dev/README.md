# Symposium Dev Extension

Development version of the Symposium Zed extension that uses your locally installed `symposium-acp-agent` instead of downloading release binaries.

## Prerequisites

Install symposium-acp-agent locally:

```bash
cargo install --path crates/symposium-acp-agent
```

## How it works

The extension downloads a tiny wrapper script that simply calls `symposium-acp-agent` from your PATH. This means you can `cargo install` new versions and they take effect immediately without updating the extension.

The wrapper archives are automatically updated on each push to main via CI.

## Installing the extension

Add this directory as a dev extension in Zed.
