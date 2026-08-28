# SproutOS deployment-template plugin protocol

Version 1 is the credential-free process boundary shared by deployment-template plugins, the
`sprout` CLI, and the SproutOS worker. The normative Rust types, JSON Schemas, and compatibility
fixtures live in this package. A consumer must pin an exact released crate version and checksum;
copying these types into another repository creates a second protocol and is not supported.

## Invocation

The runner starts the verified plugin executable with no arguments, writes exactly one UTF-8 JSON
request followed by LF to stdin, closes stdin, and reads exactly one UTF-8 JSON response from
stdout. The workspace is an absolute, normalized path in the host operating system's native path
syntax. The plugin may write bounded human diagnostics to stderr. It must not write logs, progress,
or a second JSON value to stdout.

Exit status `0` means the response has `status: "ok"`. Exit status `1` means the response has
`status: "error"`. Any other exit status, signal, timeout, missing response, oversized output, or
malformed response is a runner failure. The runner, rather than this protocol, owns isolation,
timeouts, and byte limits.

## Request

The request supplies an absolute isolated workspace plus structural metadata only:

- immutable catalogue, manifest, plugin, and upstream identities;
- the deployment preset and required capability identifiers;
- managed-service kinds and mappings from symbolic service outputs to environment names;
- user-input declarations and their environment destinations; and
- generated-input declarations and their environment destinations.

It never contains an input value, generated value, secret reference, customer or organization ID,
GitHub credential, SproutOS token, cloud credential, or provisioned service identifier. All objects
are closed: a field such as `value`, `secret`, or `credentials` is a malformed request rather than a
future extension.

The plugin verifies that the checked-out repository is the exact `upstream_commit`. It must not use
the network, GitHub, SproutOS, or customer credentials. It changes files only inside the workspace
and never commits or pushes.

## Response and diff verification

Every response carries `protocol_version: 1`. A successful response reports normalized,
forward-slash relative paths in strict lexical order. Paths may not be absolute, contain empty,
`.` or `..` components, contain backslashes, or address `.git`. Each entry reports whether the file
was created, modified, or deleted and its exact before/after SHA-256 digest. Created entries have no
before digest, deleted entries have no after digest, and modified entries have both.

The response is a claim, not proof. The runner independently snapshots the workspace before and
after execution, rejects unreported changes or digest mismatches, and enforces path and symlink
containment. On an already transformed exact workspace, applying the same request succeeds with
`changes: []` and leaves the complete tree byte-identical.

Errors use the closed codes defined by `ErrorCode`. Messages must be actionable but must not contain
environment values, credentials, file contents, or unbounded debug data.

## Compatibility

Protocol v1 never gains fields or enum variants in place. A future extension uses a new protocol
version, schema, fixtures, and Rust types. Valid and invalid v1 fixtures are release compatibility
tests: changing how an existing fixture decodes, validates, or serializes is a breaking change.
