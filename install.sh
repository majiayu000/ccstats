#!/bin/sh
# ccstats installer
# Usage: curl -fsSL https://raw.githubusercontent.com/majiayu000/ccstats/main/install.sh | sh

set -e

REPO="majiayu000/ccstats"
BINARY="ccstats"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
LATEST_RELEASE_API="https://api.github.com/repos/${REPO}/releases/latest"

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Required command not found: $1"
        exit 1
    fi
}

curl_fetch() {
    url="$1"
    output="$2"
    curl \
        --proto '=https' \
        --tlsv1.2 \
        --retry 3 \
        --retry-delay 1 \
        --connect-timeout 10 \
        --max-time 120 \
        -fsSL \
        "$url" \
        -o "$output"
}

sha256_file() {
    file="$1"
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
        return 0
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
        return 0
    fi
    return 1
}

# Detect OS and architecture
detect_platform() {
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        darwin) os="apple-darwin" ;;
        linux) os="unknown-linux-gnu" ;;
        mingw*|msys*|cygwin*) os="pc-windows-msvc" ;;
        *) echo "Unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) echo "Unsupported architecture: $arch"; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

# Get latest release version from GitHub API
get_latest_version() {
    tmp_json="$1"
    curl_fetch "$LATEST_RELEASE_API" "$tmp_json"
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp_json" | head -n 1
}

verify_checksum_if_available() {
    archive_url="$1"
    archive_path="$2"
    checksum_path="$3"
    checksum_url="${archive_url}.sha256"

    if curl_fetch "$checksum_url" "$checksum_path"; then
        expected="$(awk '{print $1}' "$checksum_path" | head -n 1)"
        if [ -z "$expected" ]; then
            echo "Warning: checksum file is empty, skipping verification."
            return 0
        fi

        if actual="$(sha256_file "$archive_path")"; then
            if [ "$actual" != "$expected" ]; then
                echo "Checksum verification failed."
                echo "Expected: $expected"
                echo "Actual:   $actual"
                exit 1
            fi
            echo "Checksum verified."
        else
            echo "Warning: no SHA-256 tool found, skipping checksum verification."
        fi
    else
        echo "Checksum file not found for this release asset, skipping verification."
    fi
}

main() {
    require_cmd curl
    require_cmd sed
    require_cmd awk
    require_cmd find

    platform=$(detect_platform)
    tmp_dir=$(mktemp -d)
    tmp_json="$tmp_dir/release.json"
    archive_path=""
    checksum_path="$tmp_dir/ccstats.sha256"

    cleanup() {
        rm -rf "$tmp_dir"
    }
    trap cleanup EXIT INT TERM

    version="$(get_latest_version "$tmp_json")"
    if [ -z "$version" ]; then
        echo "Failed to get latest version."
        exit 1
    fi

    echo "Installing ccstats ${version} for ${platform}..."

    # Determine file extension
    case "$platform" in
        *windows*) ext="zip" ;;
        *) ext="tar.gz" ;;
    esac

    url="https://github.com/${REPO}/releases/download/${version}/${BINARY}-${platform}.${ext}"
    archive_path="$tmp_dir/ccstats.${ext}"

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    echo "Downloading from ${url}..."
    curl_fetch "$url" "$archive_path"
    if [ ! -s "$archive_path" ]; then
        echo "Downloaded file is empty."
        exit 1
    fi

    verify_checksum_if_available "$url" "$archive_path" "$checksum_path"

    case "$ext" in
        tar.gz)
            require_cmd tar
            tar xzf "$archive_path" -C "$tmp_dir"
            ;;
        zip)
            require_cmd unzip
            unzip -q "$archive_path" -d "$tmp_dir"
            ;;
    esac

    bin_path="$(find "$tmp_dir" -type f -name "$BINARY" | head -n 1)"
    if [ -z "$bin_path" ]; then
        echo "Failed to locate extracted binary: $BINARY"
        exit 1
    fi

    if command -v install >/dev/null 2>&1; then
        install -m 755 "$bin_path" "$INSTALL_DIR/$BINARY"
    else
        cp "$bin_path" "$INSTALL_DIR/$BINARY"
        chmod +x "$INSTALL_DIR/$BINARY"
    fi

    echo ""
    echo "ccstats installed to $INSTALL_DIR/$BINARY"
    echo ""

    # Check if in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo "Add this to your shell profile:"
            echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
            ;;
    esac
}

main "$@"
