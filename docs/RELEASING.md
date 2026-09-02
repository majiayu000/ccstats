# Releasing ccstats

Releases are tag-driven. A successful release builds CLI binaries and desktop
installers in parallel, publishes the crate, then creates the GitHub Release
and updates the Homebrew formula. Crate publish waits for both CLI and desktop
artifacts so a failed installer build cannot ship a crate without matching
GitHub assets. Homebrew still waits for the crates.io `.crate` to exist.

## Desktop installers

The same `v*` tag also builds desktop installers and attaches them to the
GitHub Release:

- macOS: `ccstats-desktop-aarch64-apple-darwin.dmg` and
  `ccstats-desktop-x86_64-apple-darwin.dmg`
- Windows: `ccstats-desktop-x86_64-pc-windows-msvc.msi`
- Linux: `ccstats-desktop-x86_64-unknown-linux-gnu.AppImage` and
  `ccstats-desktop-aarch64-unknown-linux-gnu.AppImage`

Each installer has a matching `.sha256` sidecar. The workflow builds unsigned
or ad-hoc packages when signing secrets are missing so the pipeline can be
tested without certificates. Gatekeeper and SmartScreen stay noisy until the
secrets below are set.

Do not enable app sandboxing. The desktop app reads local agent logs under the
user home directory.

### Signing secrets

Configure these GitHub Actions secrets on `majiayu000/ccstats`. Leave them
unset to publish ad-hoc macOS DMGs and unsigned Windows MSIs.

#### macOS Developer ID + notarization

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Password for that `.p12` |
| `APPLE_SIGNING_IDENTITY` | Identity string from `security find-identity -v -p codesigning`, for example `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID` | Apple ID email |
| `APPLE_PASSWORD` | App-specific password, not the account password |
| `APPLE_TEAM_ID` | 10-character Team ID |

All six are required for a notarized DMG. A Developer ID certificate without
notarization credentials will fail the macOS desktop job.

#### Windows Authenticode

| Secret | Value |
| --- | --- |
| `WINDOWS_CERTIFICATE` | Base64-encoded `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | Export password for that `.pfx` |

The Windows job imports the certificate into the runner certificate store and
passes the thumbprint into Tauri for the MSI. Linux AppImages are checksummed
only; do not add a GPG signing step in this workflow.

## One-time crates.io setup

Open the `ccstats` crate settings on crates.io, add a GitHub trusted publisher,
and use these values before creating a release tag:

- Repository owner: `majiayu000`
- Repository name: `ccstats`
- Workflow filename: `release.yml`
- Environment: leave blank (the workflow does not declare one)

The workflow uses the official
[`rust-lang/crates-io-auth-action`](https://github.com/rust-lang/crates-io-auth-action)
to exchange GitHub OIDC identity for a short-lived crates.io token. Do not add
a long-lived crates.io API token to GitHub Secrets.

The existing `HOMEBREW_TAP_TOKEN` secret must retain permission to update
`majiayu000/homebrew-tap`.

## Release checklist

1. Update the version in `Cargo.toml`, `Cargo.lock`, `desktop/package.json`,
   `desktop/package-lock.json`, `desktop/src-tauri/Cargo.toml`,
   `desktop/src-tauri/Cargo.lock`, and `desktop/src-tauri/tauri.conf.json`.
   `scripts/check-release.sh` rejects a tag if any of these drift.
2. Move the relevant entries from `Unreleased` into a dated version section in
   `CHANGELOG.md`.
3. Run the same preflight checks used by CI:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   cargo deny check
   cargo publish --dry-run --locked
   scripts/check-release.sh
   python3 scripts/stage-desktop-artifacts.py --self-test
   ```

4. Create and push the matching tag, for example `v0.5.1` for version `0.5.1`.
5. Confirm every job in the Release workflow succeeds.

## Public verification

Verify the independently published surfaces instead of treating a green
workflow as sufficient:

```bash
cargo search ccstats --limit 1
gh release view v0.5.1 --repo majiayu000/ccstats
gh release view v0.5.1 --repo majiayu000/ccstats --json assets --jq '.assets[].name'
brew update
brew info majiayu000/tap/ccstats
cargo binstall ccstats --no-confirm
ccstats --version
```

Confirm the GitHub Release includes both CLI archives and the five desktop
installers plus checksums. On macOS, a notarized build reports
`source=Notarized Developer ID` from `spctl --assess --verbose`.

crates.io indexing and Homebrew tap updates can lag briefly. The published
version, release assets, and formula must all agree before announcing a
release.
