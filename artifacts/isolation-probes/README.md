# Native isolation proof plugins

These three artifacts exercise the production `applyTemplate` process boundary without entering
the deployment catalogue:

- `success` writes `allowed=ok`, emits a canonical protocol response, and on Linux asserts the
  exact filesystem, environment, descriptor, mount-capability, and metadata-network boundary from
  the locked SproutOS fixture. Native CI may skip those Linux-only boundary assertions only by
  explicitly setting `SPROUT_ISOLATION_NATIVE_SMOKE=1`; production clears the environment.
- `stdout-flood` writes more than 4 MiB and remains alive beyond the 120-second production limit.
- `fork-timeout` starts a real descendant. Both processes outlive 120 seconds, and the descendant
  attempts to write `descendant-survived` after 130 seconds if process-tree containment fails.

The dependency-free nested Cargo workspace produces real native executables for Linux amd64 and
arm64, macOS amd64 and arm64, and Windows amd64. The `smoke` feature shortens destructive timing for
native CI and makes the descendant proof self-cleaning. Publication always builds with
`--no-default-features`, records an empty feature list in build metadata, and never publishes smoke
bytes.

Each proof is a normal five-platform SproutOS template-plugin OCI index in its own public GHCR
repository: `isolation-proof-success`, `isolation-proof-stdout-flood`, and
`isolation-proof-fork-timeout`. Root indexes and platform manifests carry the standard template
plugin media types, the Deployment-Templates source and Apache-2.0 annotations, and a proof-kind
annotation. Each manifest contains exactly one `plugin` or `plugin.exe` executable layer.

The canonical `.github/workflows/publish.yml@refs/heads/main` identity signs and attests the three
root digests. The proof layouts and metadata remain separate from `release/subjects.json`,
`catalogue/plugin-lock.json`, application manifests, and catalogue goldens.
