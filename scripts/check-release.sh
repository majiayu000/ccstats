#!/bin/sh
# Validate release metadata before building release artifacts.

set -eu

manifest_version() {
    sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)"/\1/p' Cargo.toml | head -n 1
}

lock_version() {
    awk '
        /^\[\[package\]\]/ { in_pkg = 0; name = ""; version = "" }
        /^name = "ccstats"$/ { in_pkg = 1; name = "ccstats" }
        in_pkg && /^version = / {
            gsub(/version = "/, "");
            gsub(/"/, "");
            print;
            exit;
        }
    ' Cargo.lock
}

version="$(manifest_version)"
if [ -z "$version" ]; then
    echo "Failed to read package version from Cargo.toml" >&2
    exit 1
fi

locked_version="$(lock_version)"
if [ "$locked_version" != "$version" ]; then
    echo "Cargo.lock version ($locked_version) does not match Cargo.toml version ($version)." >&2
    exit 1
fi

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
