# Security policy

Please report suspected vulnerabilities privately through GitHub Security Advisories for this
repository. Do not open a public issue containing an exploit, credential, or unreleased finding.

Deployment-template code runs against customer source, so path traversal, symlink escape,
unreported file changes, secret exposure, mutable artifact references, provenance bypass, and
non-idempotent output are security issues. Reports should include the affected template ID,
protocol version, plugin or catalogue digest, upstream commit, and a minimal reproduction when it
is safe to provide one.

No plugin should receive customer, GitHub, cloud, registry, or SproutOS credentials. If logs or a
response appear to contain any such value, stop using the artifact and report it immediately.
