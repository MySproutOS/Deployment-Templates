# Catalogue generator

`catalogue-generator` turns the reviewable `apps/*/manifest-source.json` source specifications and
a release-produced plugin lock into the authoritative deployment-template catalogue.

A source specification is deliberately **not** a publishable app manifest. In particular, its
`plugin` object contains only `protocol_version`; a source specification that tries to author an OCI
repository, reference, or digest is rejected. The publish workflow builds each plugin first and
writes `catalogue/plugin-lock.json` with an immutable
`ghcr.io/mysproutos/<id>-plugin@sha256:<64 lowercase hex>` artifact. The generator injects the bare
repository and digest into each generated app manifest and validates that result against the
versioned schemas.

The release command is:

```text
cargo run -p catalogue-generator -- \
  --plugin-lock catalogue/plugin-lock.json \
  --output catalogue/catalogue.json \
  --provenance-output catalogue/provenance.json \
  --source-repository MySproutOS/Deployment-Templates \
  --source-workflow .github/workflows/publish.yml \
  --source-ref refs/heads/main \
  --source-commit <40-character-commit>
```

Catalogue and provenance JSON use RFC 8785 JCS plus one terminal line feed. The catalogue does not
contain its provenance digest. The detached provenance instead subjects the exact catalogue blob
and records deterministic materials for source specifications, plugin artifacts and source trees,
the plugin lock, the catalogue schemas, and the canonical template-protocol schemas and source.
This preserves an acyclic digest graph; the enclosing OCI artifact receives its separate GitHub
attestation after publication.

`live` readiness also requires detached, byte-bound end-to-end evidence matching the exact app,
upstream commit, plugin digest, preset, and required capabilities. The initial Umami and Memos
sources are blocked and contain no E2E evidence.
