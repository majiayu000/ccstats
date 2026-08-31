# Releasing ccstats

Releases are tag-driven. A successful release publishes the crate first, then
creates the multi-platform GitHub Release and updates the Homebrew formula.
This ordering prevents the Homebrew formula from pointing at a crate version
that does not exist.

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
2. Move the relevant entries from `Unreleased` into a dated version section in
   `CHANGELOG.md`.
3. Run the same preflight checks used by CI:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   cargo deny check
   cargo publish --dry-run --locked
   ```

4. Create and push the matching tag, for example `v0.5.1` for version `0.5.1`.
5. Confirm every job in the Release workflow succeeds.

## Public verification

Verify the independently published surfaces instead of treating a green
workflow as sufficient:

```bash
cargo search ccstats --limit 1
gh release view v0.5.1 --repo majiayu000/ccstats
brew update
brew info majiayu000/tap/ccstats
cargo binstall ccstats --no-confirm
ccstats --version
```

crates.io indexing and Homebrew tap updates can lag briefly. The published
version, release assets, and formula must all agree before announcing a
release.
