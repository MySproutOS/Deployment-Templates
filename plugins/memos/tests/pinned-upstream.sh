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
    "capabilities": ["generic_web", "object_storage", "provided_al2023", "realtime_sse", "serialized_startup_migrations"]
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
  "generated_inputs": []
}
JSON

first="$scratch/first.json"
second="$scratch/second.json"
"$plugin" <"$request" >"$first"
grep -Fq '"status":"ok"' "$first"
grep -Fq '"path":"cmd/memos/main.go","kind":"modified"' "$first"
test "$(sha256sum "$checkout/cmd/memos/main.go" | cut -d ' ' -f 1)" = \
  "d7e331555f9cdabcd2af749d39b62eac9962d8406cd118a2c3b12f2e088716be"

actual_status="$scratch/status"
expected_status="$scratch/expected-status"
git -C "$checkout" status --porcelain=v1 --untracked-files=all | LC_ALL=C sort >"$actual_status"
cat >"$expected_status" <<'STATUS'
 M cmd/memos/main.go
?? .config/sproutos.toml
?? sproutos/build.sh
?? sproutos/run.sh
?? store/sproutos_deployment_config.go
STATUS
diff -u "$expected_status" "$actual_status"

"$plugin" <"$request" >"$second"
grep -Fq '"changes":[]' "$second"

test -z "$(gofmt -d "$checkout/store/sproutos_deployment_config.go")"
(
  cd "$checkout"
  go test ./store
  sh sproutos/build.sh
  file .sproutos/dist/memos | grep -Fq "ARM aarch64"
)
