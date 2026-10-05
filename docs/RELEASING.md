# Releasing ccstats

Releases are tag-driven. A successful release builds CLI binaries,
publishes the crate, then creates the GitHub Release and updates the Homebrew
formula. Crate publish waits for every CLI build so a failed platform build
cannot ship a crate without matching GitHub assets. Homebrew still waits for
the crates.io `.crate` to exist.

The ccstats desktop app under `desktop/` is deprecated and is not built or
attached to releases. The GUI surface is
[QuotaBar](https://github.com/majiayu000/quotabar), released from its own
repository.

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

1. Update the version in `Cargo.toml` and `Cargo.lock`.
   `scripts/check-release.sh` rejects a tag if they drift.
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

Confirm the GitHub Release includes every CLI archive plus checksums.

crates.io indexing and Homebrew tap updates can lag briefly. The published
version, release assets, and formula must all agree before announcing a
release.
