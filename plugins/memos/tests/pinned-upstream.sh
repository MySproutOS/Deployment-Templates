#!/usr/bin/env bash
set -euo pipefail

repository="https://github.com/usememos/memos"
commit="22a5f3385b9fc5bdf282eb597aa3db79798aa5ab"
root="$(git rev-parse --show-toplevel)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/sprout-memos-upstream.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT
checkout="$scratch/memos"

git init --quiet "$checkout"
git -C "$checkout" remote add origin "$repository"
git -C "$checkout" fetch --quiet --depth=1 origin "$commit"
git -C "$checkout" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$checkout" rev-parse HEAD)" = "$commit"

cargo build --locked -p sprout-template-memos --manifest-path "$root/Cargo.toml"
plugin="$root/target/debug/sprout-template-memos"
request="$scratch/request.json"
cat >"$request" <<JSON
{
  "protocol_version": 1,
  "workspace": "$checkout",
  "template": {
    "id": "memos",
    "catalogue_digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    "manifest_digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
    "plugin_digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
    "upstream_repository": "$repository",
    "upstream_commit": "$commit"
  },
  "deployment": {
    "preset": "web",
    "capabilities": ["controlled_migrations", "generic_web", "object_storage", "provided_al2023"]
  },
  "services": [
    {
      "key": "object_storage",
      "kind": "object_storage",
      "bindings": [
        {"environment": "S3_ACCESS_KEY_ID", "output": "access_key_id"},
        {"environment": "S3_BUCKET_NAME", "output": "bucket"},
        {"environment": "S3_ENDPOINT", "output": "endpoint"},
        {"environment": "S3_FORCE_PATH_STYLE", "output": "force_path_style"},
        {"environment": "S3_REGION", "output": "region"},
        {"environment": "S3_SECRET_ACCESS_KEY", "output": "secret_access_key"}
      ]
    },
    {
      "key": "postgres",
      "kind": "postgres",
      "bindings": [{"environment": "MEMOS_DSN", "output": "connection_url"}]
    }
  ],
  "user_inputs": [],
  "generated_inputs": [
    {
      "key": "admin_password",
      "generator": "random_base64url",
      "bytes": 32,
      "environment": "MEMOS_ADMIN_PASSWORD"
    }
  ]
}
JSON

first="$scratch/first.json"
second="$scratch/second.json"
"$plugin" <"$request" >"$first"
grep -Fq '"status":"ok"' "$first"
grep -Fq '"path":".github/workflows/sproutos-deploy.yml","kind":"created"' "$first"
grep -Fq '"path":"cmd/memos/main.go","kind":"modified"' "$first"
test "$(sha256sum "$checkout/cmd/memos/main.go" | cut -d ' ' -f 1)" = \
  "ece466288c4adb1adadc89ce537d29ac25180dc6fef3d3955ecd893b65f37d92"
test "$(sha256sum "$checkout/web/src/hooks/useLiveMemoRefresh.ts" | cut -d ' ' -f 1)" = \
  "05b2cbef1a164ea21c2aecdc9c6cb1522069bf914815baf4fcb000f88cf31067"
if grep -Eq 'EventSource|fetch\("/api/v1/sse"' "$checkout/web/src/hooks/useLiveMemoRefresh.ts"; then
  echo "generated live-refresh hook still opens an SSE connection" >&2
  exit 1
fi

actual_status="$scratch/status"
expected_status="$scratch/expected-status"
git -C "$checkout" status --porcelain=v1 --untracked-files=all | LC_ALL=C sort >"$actual_status"
cat >"$expected_status" <<'STATUS'
 M cmd/memos/main.go
 M web/src/hooks/useLiveMemoRefresh.ts
?? .config/sproutos.toml
?? .github/workflows/sproutos-deploy.yml
?? cmd/sproutos-migrate/main.go
?? cmd/sproutos-migrate/main_test.go
?? sproutos/build.sh
?? sproutos/run.sh
?? store/sproutos_deployment_config.go
?? web/tests/sproutos-live-polling.test.ts
STATUS
diff -u "$expected_status" "$actual_status"

"$plugin" <"$request" >"$second"
grep -Fq '"changes":[]' "$second"

test -z "$(gofmt -d \
  "$checkout/store/sproutos_deployment_config.go" \
  "$checkout/cmd/sproutos-migrate/main.go" \
  "$checkout/cmd/sproutos-migrate/main_test.go")"
(
  cd "$checkout"
  test "$(uname -s)" = "Linux"
  test "$(uname -m)" = "aarch64"
  go test ./store ./cmd/sproutos-migrate
  sh sproutos/build.sh
  corepack pnpm@11.0.1 --dir web exec vitest run tests/sproutos-live-polling.test.ts
  file .sproutos/dist/memos | grep -Fq "ARM aarch64"
  file .sproutos/dist/memos | grep -Fq "statically linked"
  test -x .sproutos/dist/bootstrap
  file .sproutos/migration/bootstrap | grep -Fq "ARM aarch64"
  file .sproutos/migration/bootstrap | grep -Fq "statically linked"
  .sproutos/dist/memos --help >/dev/null
)
