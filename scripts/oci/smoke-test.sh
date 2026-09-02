#!/usr/bin/env bash
set -euo pipefail

command -v oras >/dev/null
command -v jq >/dev/null
command -v sha256sum >/dev/null

root="$(git rev-parse --show-toplevel)"
commit="0123456789abcdef0123456789abcdef01234567"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/sprout-oci-smoke.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT

cargo build --locked --quiet -p catalogue-generator
generator="$root/target/debug/catalogue-generator"

mkdir -p "$scratch/packages" "$scratch/catalogue"
cp -R "$root/apps" "$root/schema" "$root/plugins" "$scratch/"
cp -R "$root/packages/sprout-template-protocol" "$scratch/packages/"
# This smoke test deliberately assembles synthetic plugin binaries, so their OCI
# digests cannot match production evidence. Keep the copied catalogue sources
# blocked here; the generator suite separately validates the checked-in live
# manifests against their exact detached evidence and production plugin digests.
for manifest in "$scratch"/apps/*/manifest-source.json; do
  jq '.readiness = {
    status: "blocked",
    blocked_reasons: ["Offline OCI smoke uses synthetic plugin binaries."]
  }' "$manifest" >"$manifest.tmp"
  mv "$manifest.tmp" "$manifest"
done
if [[ -d "$root/catalogue/e2e-proofs" ]]; then
  cp -R "$root/catalogue/e2e-proofs" "$scratch/catalogue/"
fi
cp "$root/scripts/oci/assemble-release.sh" "$scratch/assemble-release.sh"

platforms=(
  'x86_64-unknown-linux-musl linux amd64 '
  'aarch64-unknown-linux-musl linux arm64 '
  'x86_64-apple-darwin darwin amd64 '
  'aarch64-apple-darwin darwin arm64 '
  'x86_64-pc-windows-msvc windows amd64 .exe'
)

for id in memos umami; do
  for entry in "${platforms[@]}"; do
    read -r target os arch suffix <<<"$entry"
    output="$scratch/dist/plugins/$id/$target"
    mkdir -p "$output"
    printf 'sprout-oci-smoke:%s:%s\n' "$id" "$target" >"$output/plugin${suffix:-}"
    digest="$(sha256sum "$output/plugin${suffix:-}" | awk '{print $1}')"
    jq -cn \
      --arg id "$id" \
      --arg target "$target" \
      --arg os "$os" \
      --arg arch "$arch" \
      --arg suffix "${suffix:-}" \
      --arg digest "sha256:$digest" \
      '{schemaVersion:1,id:$id,target:$target,os:$os,arch:$arch,suffix:$suffix,binaryDigest:$digest}' \
      >"$output/metadata.json"
  done
done

cd "$scratch"
export GITHUB_REPOSITORY=MySproutOS/Deployment-Templates
export GITHUB_REF=refs/heads/main

assemble() {
  ./assemble-release.sh \
    dist/plugins \
    release \
    ghcr.io/mysproutos \
    https://github.com/MySproutOS/Deployment-Templates \
    "$commit" \
    "$generator"
}

assemble
cp release/subjects.json "$scratch/first-subjects.json"
cp catalogue/catalogue.json "$scratch/first-catalogue.json"
assemble

cmp -s "$scratch/first-subjects.json" release/subjects.json
cmp -s "$scratch/first-catalogue.json" catalogue/catalogue.json
jq -e '
  .sourceCommit == "0123456789abcdef0123456789abcdef01234567"
  and ([.subjects[].id] | sort) == ["catalogue", "memos", "umami"]
  and all(.subjects[]; .digest | test("^sha256:[0-9a-f]{64}$"))
' release/subjects.json >/dev/null

for id in memos umami; do
  oras manifest fetch --oci-layout "release/oci/$id:sha-$commit" \
    | jq -e '
      .artifactType == "application/vnd.sproutos.template-plugin.index.v1"
      and ([.manifests[].platform | (.os + "/" + .architecture)] | sort)
        == ["darwin/amd64", "darwin/arm64", "linux/amd64", "linux/arm64", "windows/amd64"]
    ' >/dev/null
done

oras manifest fetch --oci-layout "release/oci/catalogue:sha-$commit" \
  | jq -e '.artifactType == "application/vnd.sproutos.deployment-catalogue.v1"' >/dev/null
