# GitHub Actions Reusable Workflow for Cross-Platform Rust Releases

Research on building a reusable GitHub workflow for cross-platform Rust binary releases.

## Architecture Validation

Reusable workflows fully support multiple jobs with different runners, internal matrix strategies, and coordinated uploads to the same release. Each job can independently specify `runs-on: ubuntu-latest`, `macos-14`, or `windows-latest`.

For release uploads, two coordination patterns are proven in production:

- **Pattern 1 (Recommended)**: Create release first in a dedicated job, then fan out build jobs with `needs: create-release`. Each build job uploads to the `upload_url` output.
- **Pattern 2**: Use `softprops/action-gh-release` which handles concurrent uploads atomically.

## Cargo.toml Parsing

Use `cargo metadata --format-version=1 --no-deps | jq`:

| Approach | Reliability | Custom Metadata | Cross-Platform |
|----------|-------------|-----------------|----------------|
| cargo metadata + jq | ⭐⭐⭐⭐⭐ | Full access | All platforms |
| dasel | ⭐⭐⭐⭐⭐ | Full access | All platforms |
| toml-cli | ⭐⭐⭐⭐ | Full access | Build required |
| grep/sed/awk | ⭐⭐ | Unreliable | BSD/GNU issues |

Custom `[package.metadata.symposium]` sections appear in the JSON output under `packages[0].metadata.symposium`:

```yaml
- name: Extract package metadata
  id: meta
  shell: bash
  run: |
    METADATA=$(cargo metadata --format-version=1 --no-deps)
    echo "name=$(echo "$METADATA" | jq -r '.packages[0].name')" >> $GITHUB_OUTPUT
    echo "version=$(echo "$METADATA" | jq -r '.packages[0].version')" >> $GITHUB_OUTPUT
    echo "binary=$(echo "$METADATA" | jq -r '.packages[0].metadata.symposium.binary // ""')" >> $GITHUB_OUTPUT
    echo "args=$(echo "$METADATA" | jq -c '.packages[0].metadata.symposium.args // []')" >> $GITHUB_OUTPUT
```

Both `cargo` and `jq` are pre-installed on all GitHub-hosted runners.

## Why Not cargo-dist or cross-rs

**cargo-dist** generates complete, self-contained workflows rather than providing reusable components. It's incompatible with the reusable workflow pattern where callers `uses: org/repo/.github/workflows/build.yml@v1`.

**cross-rs** cannot practically build macOS from Linux (requires SDK extraction, custom Docker images, legal gray areas). Every major Rust project uses native macOS runners for Darwin targets.

For Linux ARM targets, native ARM runners (`ubuntu-24.04-arm`) are now free for public repos, so native builds are simpler than cross-rs.

## Critical Implementation Details

### Permissions and Secrets

The reusable workflow cannot request `contents: write` - callers must set it:

```yaml
jobs:
  release:
    permissions:
      contents: write  # Required - cannot be set by called workflow
    uses: symposium-dev/package-agent-mod/.github/workflows/build.yml@v1
    secrets: inherit
```

`secrets: inherit` only works within the same organization. For cross-org callers, secrets must be explicitly declared.

### Parallel Upload Coordination

Multiple jobs uploading to the same release can cause 409 Conflict errors. Use the two-phase pattern:

```yaml
jobs:
  create-release:
    runs-on: ubuntu-latest
    outputs:
      upload_url: ${{ steps.create.outputs.upload_url }}
    steps:
      - uses: softprops/action-gh-release@v2
        id: create
        with:
          draft: true
          files: ""

  build:
    needs: create-release
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-musl
            os: ubuntu-latest
          - target: aarch64-apple-darwin
            os: macos-14
    runs-on: ${{ matrix.os }}
```

### Windows MAX_PATH Limits

Enable long paths for Windows builds:

```yaml
- name: Enable long paths (Windows)
  if: runner.os == 'Windows'
  run: git config --system core.longpaths true
```

### musl Allocator Performance

musl's memory allocator is 7-20x slower than glibc's under multi-threaded workloads. For performance-sensitive binaries, override with jemalloc:

```rust
#[cfg(target_env = "musl")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

## Patterns from Production Projects

Analysis of ripgrep, bat, fd, delta, nushell, and hyperfine:

- **Two-phase release structure is universal**: `create-release` → `build-release` → optional `publish-release`
- **Naming convention**: `{binary}-{target}-{version}.{ext}` with `.tar.gz` for Unix and `.zip` for Windows
- **Workflow versioning**: Use major tags (`v1`, `v2`) as floating tags pointing to latest patch

## Recommended Workflow Structure

```yaml
name: Build and Release Mod

on:
  workflow_call:
    inputs:
      manifest:
        description: 'Path to Cargo.toml'
        type: string
        default: './Cargo.toml'
      musl:
        description: 'Use musl for Linux builds (true) or glibc (false)'
        type: boolean
        required: true

jobs:
  metadata:
    runs-on: ubuntu-latest
    outputs:
      name: ${{ steps.meta.outputs.name }}
      version: ${{ steps.meta.outputs.version }}
      binary: ${{ steps.meta.outputs.binary }}
    steps:
      - uses: actions/checkout@v4
      - name: Extract metadata
        id: meta
        run: |
          METADATA=$(cargo metadata --format-version=1 --no-deps --manifest-path ${{ inputs.manifest }})
          echo "name=$(echo "$METADATA" | jq -r '.packages[0].name')" >> $GITHUB_OUTPUT
          echo "version=$(echo "$METADATA" | jq -r '.packages[0].version')" >> $GITHUB_OUTPUT
          echo "binary=$(echo "$METADATA" | jq -r '.packages[0].metadata.symposium.binary // .packages[0].name')" >> $GITHUB_OUTPUT

  build:
    needs: metadata
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-${{ inputs.musl && 'musl' || 'gnu' }}
            os: ubuntu-latest
          - target: aarch64-unknown-linux-${{ inputs.musl && 'musl' || 'gnu' }}
            os: ubuntu-24.04-arm
          - target: x86_64-apple-darwin
            os: macos-13
          - target: aarch64-apple-darwin
            os: macos-14
          - target: x86_64-pc-windows-msvc
            os: windows-latest
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      - name: Package
        # Create {binary}-{os}-{arch}-{version}.zip
      - uses: softprops/action-gh-release@v2
        with:
          files: '*.zip'
```

## Caller Template

```yaml
# .github/workflows/release.yml
on:
  release:
    types: [published]

jobs:
  release:
    permissions:
      contents: write
    uses: symposium-dev/package-agent-mod/.github/workflows/build.yml@v1
    with:
      musl: true
    secrets: inherit
```
