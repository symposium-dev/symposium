#!/bin/bash
# Build wrapper archives for the Symposium Dev Zed extension

set -e
cd "$(dirname "$0")"

# Darwin (macOS) - same script works for both architectures
tar -czvf symposium-dev-darwin.tar.gz symposium-dev

# Linux - same script works for both architectures
tar -czvf symposium-dev-linux.tar.gz symposium-dev

echo "Built archives:"
ls -la *.tar.gz
