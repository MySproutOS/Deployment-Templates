# SproutOS Deployment Templates

This repository is the canonical, public source for SproutOS App Store catalogue metadata and
deployment-template plugins. Catalogue entries select an exact upstream commit and an immutable
plugin artifact. They never discover templates from files in an application repository.

The first recipes target:

- Umami at `ca661c7057984aa98ed4f7083d84dae2f65bfcb0`.
- Memos at `22a5f3385b9fc5bdf282eb597aa3db79798aa5ab`.

Both entries are intentionally blocked from publication as live listings until SproutOS exposes
their declared capabilities and the pinned recipe completes a real end-to-end deployment. A green
unit test is not live-deployment evidence.

## Trust boundary

The versioned contract in `packages/sprout-template-protocol` is the only input a plugin accepts.
It contains public provenance and structural declarations, never user values, generated secrets,
service credentials, SproutOS credentials, or GitHub credentials. A plugin receives an isolated
workspace, makes deterministic file changes, and reports each changed path and byte digest. The
caller must independently compare that report with the actual filesystem diff.

Published plugin artifacts are addressed by OCI digest. The generated catalogue is canonical and
records those immutable digests plus its source provenance. Release workflows use GitHub OIDC for
keyless signatures and attestations; no long-lived signing key belongs in this repository.

## Repository layout

| Path | Purpose |
| --- | --- |
| `apps/` | Reviewed source specifications for catalogue entries |
| `catalogue/` | Plugin digest lock and generated catalogue/provenance |
| `packages/sprout-template-protocol/` | Canonical JSON wire contract, schemas, and vectors |
| `packages/catalogue-generator/` | Deterministic validation and catalogue generation |
| `plugins/` | App-specific Rust transformations |
| `crates/template-runtime/` | Shared fail-closed filesystem mechanics for plugins |
| `schema/` | Closed JSON Schemas for published artifacts |

## Local verification

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Recipe integration tests clone or receive the exact upstream commits recorded in
`tests/upstream-lock.json`, apply each plugin twice, and require the second run to report no changes.
Publishing is tag- or manually gated; pull requests never push packages or catalogue artifacts.

## Release model

### Protocol 0.1.0 distribution

The supported `sprout-template-protocol` 0.1.0 distribution is the exact Git revision
`fea608ab7c8da209354e89df5fa4a98ee2cfcf45`, also named by the `protocol-v0.1.0` release tag:

```toml
sprout-template-protocol = { git = "https://github.com/MySproutOS/Deployment-Templates", rev = "fea608ab7c8da209354e89df5fa4a98ee2cfcf45", version = "=0.1.0" }
```

The GitHub release attaches the packaged crate, its `SHA256SUMS`, and GitHub OIDC-signed SLSA
provenance. Version 0.1.0 is not published to crates.io; consumers must pin the revision above and
commit Cargo's resolved source to `Cargo.lock`.

1. Review and merge protocol, recipe, and catalogue source changes.
2. Build every supported plugin target from a protected source ref.
3. Assemble one deterministic OCI artifact per plugin and record the registry-reported digest.
4. Keyless-sign and attest each artifact.
5. Generate the catalogue from the reviewed sources and exact plugin lock.
6. Publish, sign, and attest the catalogue artifact.

SproutOS imports only a verified generated catalogue. Source specifications and mutable OCI tags
are never import authority.
