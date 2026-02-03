#!/bin/sh
# ccstats installer
# Usage: curl -fsSL https://raw.githubusercontent.com/majiayu000/ccstats/main/install.sh | sh

set -e

REPO="majiayu000/ccstats"
BINARY="ccstats"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
detect_platform() {
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    case "$OS" in
        darwin) OS="apple-darwin" ;;
        linux) OS="unknown-linux-gnu" ;;
        mingw*|msys*|cygwin*) OS="pc-windows-msvc" ;;
        *) echo "Unsupported OS: $OS"; exit 1 ;;
    esac

    case "$ARCH" in
        x86_64|amd64) ARCH="x86_64" ;;
        arm64|aarch64) ARCH="aarch64" ;;
        *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac

    echo "${ARCH}-${OS}"
}

# Get latest release version
get_latest_version() {
    curl -sL "https://api.github.com/repos/${REPO}/releases/latest" | \
        grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
}

main() {
    PLATFORM=$(detect_platform)
    VERSION=$(get_latest_version)

    if [ -z "$VERSION" ]; then
        echo "Failed to get latest version"
        exit 1
    fi

    echo "Installing ccstats ${VERSION} for ${PLATFORM}..."

    # Determine file extension
    case "$PLATFORM" in
        *windows*) EXT="zip" ;;
        *) EXT="tar.gz" ;;
    esac

    URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY}-${PLATFORM}.${EXT}"

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    # Download and extract
    TMPDIR=$(mktemp -d)
    cd "$TMPDIR"

    echo "Downloading from ${URL}..."
    curl -fsSL "$URL" -o "ccstats.${EXT}"

    case "$EXT" in
        tar.gz) tar xzf "ccstats.${EXT}" ;;
        zip) unzip -q "ccstats.${EXT}" ;;
    esac

    # Install binary
    mv "$BINARY" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/$BINARY"

    # Cleanup
    cd - > /dev/null
    rm -rf "$TMPDIR"

    echo ""
    echo "ccstats installed to $INSTALL_DIR/$BINARY"
    echo ""

    # Check if in PATH
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo "Add this to your shell profile:"
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
    fi
}

main
