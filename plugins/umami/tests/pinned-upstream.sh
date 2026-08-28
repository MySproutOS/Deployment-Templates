#!/usr/bin/env bash
set -euo pipefail

repository="https://github.com/umami-software/umami"
commit="ca661c7057984aa98ed4f7083d84dae2f65bfcb0"
root="$(git rev-parse --show-toplevel)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/sprout-umami-upstream.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT
checkout="$scratch/umami"

test "$(uname -s)" = "Linux"
test "$(uname -m)" = "aarch64"
test "$(node -p 'process.versions.node.split(".")[0]')" = "22"
test "$(pnpm --version)" = "11.21.0"

git init --quiet "$checkout"
git -C "$checkout" remote add origin "$repository"
git -C "$checkout" fetch --quiet --depth=1 origin "$commit"
git -C "$checkout" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$checkout" rev-parse HEAD)" = "$commit"

cargo build --locked -p sprout-template-umami --manifest-path "$root/Cargo.toml"
plugin="$root/target/debug/sprout-template-umami"
request="$scratch/request.json"
cat >"$request" <<JSON
{
  "protocol_version": 1,
  "workspace": "$checkout",
  "template": {
    "id": "umami",
    "catalogue_digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    "manifest_digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
    "plugin_digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
    "upstream_repository": "$repository",
    "upstream_commit": "$commit"
  },
  "deployment": {
    "preset": "next",
    "capabilities": ["controlled_migrations", "next_standalone"]
  },
  "services": [{
    "key": "postgres",
    "kind": "postgres",
    "bindings": [{"environment": "DATABASE_URL", "output": "connection_url"}]
  }],
  "user_inputs": [],
  "generated_inputs": [{
    "key": "app_secret",
    "generator": "random_base64url",
    "bytes": 32,
    "environment": "APP_SECRET"
  }]
}
JSON

first="$scratch/first.json"
second="$scratch/second.json"
"$plugin" <"$request" >"$first"
grep -Fq '"status":"ok"' "$first"
grep -Fq '"path":".github/workflows/sproutos-deploy.yml","kind":"created"' "$first"
grep -Fq '"path":"sproutos/migration/index.mjs","kind":"created"' "$first"

actual_status="$scratch/status"
expected_status="$scratch/expected-status"
git -C "$checkout" status --porcelain=v1 --untracked-files=all | LC_ALL=C sort >"$actual_status"
cat >"$expected_status" <<'STATUS'
?? .config/sproutos.toml
?? .github/workflows/sproutos-deploy.yml
?? sproutos/build-migration.mjs
?? sproutos/migration/control.json
?? sproutos/migration/index.mjs
?? sproutos/migration/npm-shrinkwrap.json
?? sproutos/migration/package.build.json
STATUS
diff -u "$expected_status" "$actual_status"

"$plugin" <"$request" >"$second"
grep -Fq '"changes":[]' "$second"

(
  cd "$checkout"
  pnpm install --frozen-lockfile
  env -u VERCEL \
    DATABASE_URL=postgresql://build:build@127.0.0.1:5432/build \
    DISABLE_TELEMETRY=1 \
    pnpm run build-docker
  node sproutos/build-migration.mjs
  test -d .next/standalone
  test -f .sproutos/build/migration/index.mjs
  test -f .sproutos/build/migration/control.json
  test -x .sproutos/build/migration/schema-engine
  test "$(sha256sum .sproutos/build/migration/schema-engine | cut -d ' ' -f 1)" = \
    "d9a989479930b10d81b7e2bd8027723ca455189f402d4323ed452ae3a7a793cf"
  node --check .sproutos/build/migration/index.mjs
)
