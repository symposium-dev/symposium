#!/usr/bin/env bash
# Symposium bootstrap and forwarding script.
# Finds an existing symposium binary or downloads one, then forwards all arguments.

set -euo pipefail

SYMPOSIUM_DIR="${HOME}/.symposium"
REPO="symposium-dev/symposium"

find_binary() {
    # 1. Check ~/.cargo/bin (cargo install / cargo binstall)
    if [ -x "${HOME}/.cargo/bin/symposium" ]; then
        echo "${HOME}/.cargo/bin/symposium"
        return 0
    fi

    # 2. Check ~/.symposium (our install location)
    if [ -x "${SYMPOSIUM_DIR}/symposium" ]; then
        echo "${SYMPOSIUM_DIR}/symposium"
        return 0
    fi

    # 3. Check PATH
    if command -v symposium >/dev/null 2>&1; then
        command -v symposium
        return 0
    fi

    return 1
}

detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "${os}" in
        Darwin)
            case "${arch}" in
                arm64|aarch64) echo "aarch64-apple-darwin" ;;
                *) echo >&2 "Unsupported macOS architecture: ${arch}"; exit 1 ;;
            esac
            ;;
        Linux)
            case "${arch}" in
                x86_64)  echo "x86_64-unknown-linux-musl" ;;
                aarch64) echo "aarch64-unknown-linux-musl" ;;
                *) echo >&2 "Unsupported Linux architecture: ${arch}"; exit 1 ;;
            esac
            ;;
        *)
            echo >&2 "Unsupported OS: ${os}"
            exit 1
            ;;
    esac
}

install_binary() {
    # 1. Try downloading a prebuilt binary
    if download_binary; then
        return 0
    fi

    # 2. Try cargo binstall (fast, prebuilt)
    if command -v cargo-binstall >/dev/null 2>&1; then
        echo >&2 "Installing symposium via cargo binstall..."
        if cargo binstall --no-confirm symposium 2>/dev/null; then
            return 0
        fi
    fi

    # 3. Last resort: cargo install (compiles from source)
    if command -v cargo >/dev/null 2>&1; then
        echo >&2 "Installing symposium via cargo install (this may take a while)..."
        cargo install symposium
        return 0
    fi

    echo >&2 "Error: could not install symposium. Install cargo or provide curl/wget."
    exit 1
}

download_binary() {
    local target url
    target="$(detect_target)" || return 1

    url="https://github.com/${REPO}/releases/latest/download/symposium-${target}.tar.gz"

    echo >&2 "Downloading symposium for ${target}..."

    DOWNLOAD_TMPDIR="$(mktemp -d)"
    trap 'rm -rf "${DOWNLOAD_TMPDIR}"' RETURN

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "${url}" -o "${DOWNLOAD_TMPDIR}/symposium.tar.gz" || return 1
    elif command -v wget >/dev/null 2>&1; then
        wget -q "${url}" -O "${DOWNLOAD_TMPDIR}/symposium.tar.gz" || return 1
    else
        return 1
    fi

    mkdir -p "${SYMPOSIUM_DIR}"
    tar -xzf "${DOWNLOAD_TMPDIR}/symposium.tar.gz" -C "${SYMPOSIUM_DIR}"
    chmod +x "${SYMPOSIUM_DIR}/symposium"

    echo >&2 "Installed symposium to ${SYMPOSIUM_DIR}/symposium"
}

# If we're in a symposium checkout, use cargo run directly
if CARGO_TOML="$(cargo locate-project --message-format plain 2>/dev/null)"; then
    if grep -q '^name = "symposium"$' "$CARGO_TOML" 2>/dev/null; then
        exec cargo run --quiet -- "$@"
    fi
fi

# Otherwise, find or install the binary
BINARY="$(find_binary)" || {
    install_binary
    BINARY="$(find_binary)" || {
        echo >&2 "Error: symposium installation succeeded but binary not found"
        exit 1
    }
}

# Forward all arguments
exec "${BINARY}" "$@"
