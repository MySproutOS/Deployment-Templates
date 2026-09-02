# SproutOS Deployment Templates

This repository is the canonical, public source for SproutOS App Store catalogue metadata and
deployment-template plugins. Catalogue entries select an exact upstream commit and an immutable
plugin artifact. They never discover templates from files in an application repository.

The first recipes target:

- Umami at `ca661c7057984aa98ed4f7083d84dae2f65bfcb0`.
- Memos at `22a5f3385b9fc5bdf282eb597aa3db79798aa5ab`.

Both entries carry detached production acceptance evidence bound to their exact upstream commit and
plugin digest. The recorded runs cover controlled migrations, serving health, generated-owner
authentication, and persistence across a second deployment; Memos additionally covers managed
object storage and its bounded visible-tab refresh adaptation. A green unit test alone is not
live-deployment evidence.

## Trust boundary

The versioned contract in `packages/sprout-template-protocol` is the only input a plugin accepts.
It contains public provenance and structural declarations, never user values, generated secrets,
service credentials, SproutOS credentials, or GitHub credentials. A plugin receives an isolated
workspace, makes deterministic file changes, and reports each changed path and byte digest. The
caller must independently compare that report with the actual filesystem diff.

Published plugin artifacts are addressed by OCI digest. The generated catalogue is canonical and
records those immutable digests plus its source provenance. Release workflows use GitHub OIDC for
keyless signatures and attestations; no long-lived signing key belongs in this repository.

After public verification and immutable GitHub release publication, the protected `main`
publication workflow mints a second short-lived OIDC token with audience `sproutos` and requests
`https://api.sproutos.me/v1/deploy/catalogue/import` for the exact catalogue OCI digest. The
delivery job rechecks the downloaded catalogue bytes, source SHA, repository, workflow, ref, and
provenance subject before requesting the import. Its only permissions are `contents: read` and
`id-token: write`; the API address is fixed so a repository variable cannot redirect the token, and
there is no SproutOS token or secret to configure. Transport retries reuse the same run identity and
digest, which the API queues idempotently.

The request does not publish listings. SproutOS pulls and verifies the digest and provenance again,
then reconciles `blocked` manifests as drafts; only manifests already carrying verified `live`
readiness are eligible to become public.

Each plugin also owns the exact `.github/workflows/sproutos-deploy.yml` installed into a generated
fork. That workflow builds the pinned application source on GitHub-hosted Actions, then invokes the
deploy action at a full commit SHA. The control-plane worker applies and pushes the deterministic
template but never installs dependencies or runs an application's build scripts. The generated
workflow deploys only from the repository default branch, grants only `contents: read` and
`id-token: write`, and exchanges GitHub OIDC for a short-lived repository-bound deploy token; it
contains no SproutOS secret or stored token. Upstream dependency installation and build scripts run
in a separate job that has no OIDC permission. Only a run-scoped artifact containing the exact build
outputs crosses into the deploy job.

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
7. Verify it anonymously, publish its immutable release, and request the digest-pinned SproutOS
   import with the protected workflow's OIDC identity.

SproutOS imports only a verified generated catalogue. Source specifications and mutable OCI tags
are never import authority.
