#!/bin/bash
# Sync extension versions to match symposium-acp-agent
#
# Usage: ./ci/sync-versions.sh <version>
# Example: ./ci/sync-versions.sh 1.3.0

set -euo pipefail

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 1.3.0"
    exit 1
fi

VERSION="$1"

# Validate version format (semver: X.Y.Z)
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in semver format (X.Y.Z), got: $VERSION"
    exit 1
fi

# Extract major.minor for Zed extension version field
MAJOR_MINOR="${VERSION%.*}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Syncing versions to $VERSION"
echo "  Repository root: $REPO_ROOT"

# Update vscode-extension/package.json
PACKAGE_JSON="$REPO_ROOT/vscode-extension/package.json"
if [ -f "$PACKAGE_JSON" ]; then
    # Use sed to update the version field (first occurrence)
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sed -i '' "s/\"version\": \"[0-9.]*\"/\"version\": \"$VERSION\"/" "$PACKAGE_JSON"
    else
        sed -i "s/\"version\": \"[0-9.]*\"/\"version\": \"$VERSION\"/" "$PACKAGE_JSON"
    fi
    echo "  Updated: $PACKAGE_JSON"
else
    echo "  Warning: $PACKAGE_JSON not found"
fi

# Update zed-extension/prod/extension.toml
EXTENSION_TOML="$REPO_ROOT/zed-extension/prod/extension.toml"
if [ -f "$EXTENSION_TOML" ]; then
    # Update the version field
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sed -i '' "s/^version = \"[0-9.]*\"/version = \"$MAJOR_MINOR\"/" "$EXTENSION_TOML"
        # Update all archive URLs to use the new version
        sed -i '' "s|/releases/download/symposium-acp-agent-v[0-9.]*|/releases/download/symposium-acp-agent-v$VERSION|g" "$EXTENSION_TOML"
    else
        sed -i "s/^version = \"[0-9.]*\"/version = \"$MAJOR_MINOR\"/" "$EXTENSION_TOML"
        sed -i "s|/releases/download/symposium-acp-agent-v[0-9.]*|/releases/download/symposium-acp-agent-v$VERSION|g" "$EXTENSION_TOML"
    fi
    echo "  Updated: $EXTENSION_TOML"
else
    echo "  Warning: $EXTENSION_TOML not found"
fi

echo "Done!"
