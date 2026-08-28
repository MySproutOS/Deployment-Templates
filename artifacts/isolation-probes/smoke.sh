#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <kind> <target> <suffix>" >&2
  exit 2
fi

kind="$1"
target="$2"
suffix="$3"
artifact_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
[[ "$kind" == success || "$kind" == stdout-flood || "$kind" == fork-timeout ]]
[[ "$suffix" == "" || "$suffix" == .exe ]]

target_dir="$(mktemp -d)"
smoke_dir="$(mktemp -d)"
trap 'rm -rf "$target_dir" "$smoke_dir"' EXIT
CARGO_TARGET_DIR="$target_dir" cargo build \
  --manifest-path "$artifact_dir/Cargo.toml" \
  --locked \
  --features smoke \
  --bin "$kind" \
  --target "$target"
binary="$target_dir/$target/debug/$kind$suffix"

case "$kind" in
  success)
    (
      cd "$smoke_dir"
      SPROUT_ISOLATION_NATIVE_SMOKE=1 "$binary" >response.json
      status="$(jq -er '.status' response.json)"
      changed_path="$(jq -er '.changes[0].path' response.json)"
      after_sha256="$(jq -er '.changes[0].after_sha256' response.json)"
      [[ "$status" == ok ]]
      [[ "$changed_path" == allowed ]]
      [[ "$after_sha256" == 2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df ]]
      [[ "$(cat allowed)" == ok ]]
    )
    ;;
  stdout-flood)
    "$binary" >"$smoke_dir/stdout"
    [[ "$(wc -c <"$smoke_dir/stdout" | tr -d ' ')" -gt 4194304 ]]
    ;;
  fork-timeout)
    (
      cd "$smoke_dir"
      "$binary"
      [[ ! -e descendant-smoke && ! -e descendant-survived ]]
    )
    ;;
esac
