#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "usage: $0 <kind> <target> <os> <arch> <suffix> <output-root>" >&2
  exit 2
fi

kind="$1"
target="$2"
os="$3"
arch="$4"
suffix="$5"
output_root="$6"
artifact_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

[[ "$kind" == success || "$kind" == stdout-flood || "$kind" == fork-timeout ]]
for value in "$target" "$os" "$arch"; do
  [[ "$value" =~ ^[A-Za-z0-9_.-]+$ ]]
done
[[ "$suffix" == "" || "$suffix" == .exe ]]
[[ "$output_root" == dist/isolation-probes ]]

workspace="$(pwd -P)"
SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
export SOURCE_DATE_EPOCH
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_DEBUG=0
RUSTFLAGS="-Cstrip=symbols --remap-path-prefix=${workspace}=."
if [[ "$target" == *-pc-windows-msvc ]]; then
  RUSTFLAGS="$RUSTFLAGS -Clink-arg=/Brepro"
fi
export RUSTFLAGS

first="$(mktemp -d)"
second="$(mktemp -d)"
trap 'rm -rf "$first" "$second"' EXIT
for target_dir in "$first" "$second"; do
  CARGO_TARGET_DIR="$target_dir" cargo build \
    --manifest-path "$artifact_dir/Cargo.toml" \
    --locked \
    --release \
    --no-default-features \
    --bin "$kind" \
    --target "$target"
done

first_binary="$first/$target/release/$kind$suffix"
second_binary="$second/$target/release/$kind$suffix"
cmp "$first_binary" "$second_binary" || {
  echo "$kind is not reproducible for $target" >&2
  exit 1
}

output="$output_root/$kind/$target"
mkdir -p "$output"
install -m 0755 "$first_binary" "$output/plugin$suffix"
if command -v sha256sum >/dev/null; then
  digest="$(sha256sum "$output/plugin$suffix" | awk '{print $1}')"
else
  digest="$(shasum -a 256 "$output/plugin$suffix" | awk '{print $1}')"
fi
jq -cn \
  --arg kind "$kind" \
  --arg target "$target" \
  --arg os "$os" \
  --arg arch "$arch" \
  --arg suffix "$suffix" \
  --arg digest "sha256:$digest" \
  '{schemaVersion:1,kind:$kind,target:$target,os:$os,arch:$arch,suffix:$suffix,binaryDigest:$digest,features:[]}' \
  >"$output/metadata.json"
