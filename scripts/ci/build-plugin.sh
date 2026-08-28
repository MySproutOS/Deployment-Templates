#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 7 ]]; then
  echo "usage: $0 <package> <bin> <id> <target> <os> <arch> <suffix>" >&2
  exit 2
fi

package="$1"
binary_name="$2"
plugin_id="$3"
target="$4"
os="$5"
arch="$6"
suffix="$7"

for value in "$package" "$binary_name" "$plugin_id" "$target" "$os" "$arch"; do
  [[ "$value" =~ ^[A-Za-z0-9_.-]+$ ]] || {
    echo "Unsafe build-matrix value: $value" >&2
    exit 1
  }
done
[[ "$suffix" == "" || "$suffix" == ".exe" ]] || {
  echo "Unexpected executable suffix: $suffix" >&2
  exit 1
}

workspace="$(pwd -P)"
source_epoch="$(git show -s --format=%ct HEAD)"
export SOURCE_DATE_EPOCH="$source_epoch"
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_DEBUG=0
export RUSTFLAGS="-Cstrip=symbols --remap-path-prefix=${workspace}=."

first="$(mktemp -d)"
second="$(mktemp -d)"
trap 'rm -rf "$first" "$second"' EXIT

build() {
  local target_dir="$1"
  CARGO_TARGET_DIR="$target_dir" cargo build \
    --locked \
    --release \
    --package "$package" \
    --bin "$binary_name" \
    --target "$target"
}

# Two independent target directories make non-reproducible output a release-blocking error.
build "$first"
build "$second"

first_binary="$first/$target/release/${binary_name}${suffix}"
second_binary="$second/$target/release/${binary_name}${suffix}"
[[ -f "$first_binary" && -f "$second_binary" ]] || {
  echo "Cargo did not produce the expected executable $binary_name$suffix" >&2
  exit 1
}
cmp -s "$first_binary" "$second_binary" || {
  echo "Reproducibility check failed for $package on $target" >&2
  if command -v sha256sum >/dev/null; then
    sha256sum "$first_binary" "$second_binary" >&2
  else
    shasum -a 256 "$first_binary" "$second_binary" >&2
  fi
  exit 1
}

output="dist/plugins/$plugin_id/$target"
mkdir -p "$output"
install -m 0755 "$first_binary" "$output/plugin${suffix}"
if command -v sha256sum >/dev/null; then
  digest="$(sha256sum "$output/plugin${suffix}" | awk '{print $1}')"
else
  digest="$(shasum -a 256 "$output/plugin${suffix}" | awk '{print $1}')"
fi

jq -cn \
  --arg schemaVersion "1" \
  --arg id "$plugin_id" \
  --arg package "$package" \
  --arg bin "$binary_name" \
  --arg target "$target" \
  --arg os "$os" \
  --arg arch "$arch" \
  --arg suffix "$suffix" \
  --arg digest "sha256:$digest" \
  '{
    schemaVersion: ($schemaVersion | tonumber),
    id: $id,
    package: $package,
    bin: $bin,
    target: $target,
    os: $os,
    arch: $arch,
    suffix: $suffix,
    binaryDigest: $digest
  }' >"$output/metadata.json"
