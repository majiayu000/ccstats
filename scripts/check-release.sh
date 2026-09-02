#!/bin/sh
# Validate release metadata before building release artifacts.

set -eu

toml_version() {
    sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)"/\1/p' "$1" | head -n 1
}

package_lock_version() {
    lockfile=$1
    package=$2
    awk -v pkg="$package" '
        /^\[\[package\]\]/ { in_pkg = 0 }
        /^name = "/ {
            name = $0
            sub(/^name = "/, "", name)
            sub(/"$/, "", name)
            in_pkg = (name == pkg)
        }
        in_pkg && /^version = "/ {
            gsub(/version = "/, "")
            gsub(/"/, "")
            print
            exit
        }
    ' "$lockfile"
}

json_top_version() {
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8"))["version"])' "$1"
}

require_same() {
    label=$1
    actual=$2
    expected=$3
    if [ -z "$actual" ]; then
        echo "Failed to read version from $label." >&2
        exit 1
    fi
    if [ "$actual" != "$expected" ]; then
        echo "$label version ($actual) does not match crate version ($expected)." >&2
        exit 1
    fi
}

version="$(toml_version Cargo.toml)"
if [ -z "$version" ]; then
    echo "Failed to read package version from Cargo.toml" >&2
    exit 1
fi

require_same "Cargo.lock" "$(package_lock_version Cargo.lock ccstats)" "$version"
require_same "desktop/src-tauri/Cargo.toml" "$(toml_version desktop/src-tauri/Cargo.toml)" "$version"
require_same "desktop/src-tauri/Cargo.lock" "$(package_lock_version desktop/src-tauri/Cargo.lock ccstats-desktop)" "$version"
require_same "desktop/src-tauri/tauri.conf.json" "$(json_top_version desktop/src-tauri/tauri.conf.json)" "$version"
require_same "desktop/package.json" "$(json_top_version desktop/package.json)" "$version"
require_same "desktop/package-lock.json" "$(json_top_version desktop/package-lock.json)" "$version"

expected_tag="v$version"
actual_tag="${GITHUB_REF_NAME:-}"
if [ -n "$actual_tag" ] && [ "$actual_tag" != "$expected_tag" ]; then
    echo "Release tag ($actual_tag) does not match package version ($expected_tag)." >&2
    exit 1
fi

if ! grep -Eq "^## \[$version\]( - |$)" CHANGELOG.md; then
    echo "CHANGELOG.md is missing a release section for [$version]." >&2
    exit 1
fi

echo "Release metadata validated for $expected_tag."
