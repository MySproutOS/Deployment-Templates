#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <built-dir> <release-dir> <registry-base> <publication-commit>" >&2
  exit 2
fi

built_dir="$1"
release_dir="$2"
registry_base="${3%/}"
publication_commit="$4"
artifact_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
workspace_root="$(pwd -P)"

[[ "$built_dir" == dist/isolation-probes ]]
[[ "$release_dir" == release/isolation-probes ]]
[[ "$registry_base" =~ ^ghcr\.io/[a-z0-9-]+$ ]]
[[ "$publication_commit" =~ ^[0-9a-f]{40}$ ]]
command -v jq >/dev/null
command -v oras >/dev/null

rm -rf "$release_dir"
mkdir -p "$release_dir/oci"
subjects='[]'
expected_platforms='["darwin/amd64","darwin/arm64","linux/amd64","linux/arm64","windows/amd64"]'
created=1970-01-01T00:00:00Z

for kind in success stdout-flood fork-timeout; do
  repository="$registry_base/isolation-proof-$kind"
  layout="$release_dir/oci/$kind"
  root_tag="sha-$publication_commit"
  platform_tags=()
  seen_platforms='[]'
  platforms='[]'

  while IFS= read -r metadata; do
    [[ "$(jq -r '.kind' "$metadata")" == "$kind" ]] || continue
    target="$(jq -r '.target' "$metadata")"
    os="$(jq -r '.os' "$metadata")"
    arch="$(jq -r '.arch' "$metadata")"
    suffix="$(jq -r '.suffix' "$metadata")"
    binary="$(dirname "$metadata")/plugin$suffix"
    expected_digest="$(jq -r '.binaryDigest' "$metadata")"
    if command -v sha256sum >/dev/null; then
      actual_digest="sha256:$(sha256sum "$binary" | awk '{print $1}')"
    else
      actual_digest="sha256:$(shasum -a 256 "$binary" | awk '{print $1}')"
    fi
    [[ "$actual_digest" == "$expected_digest" ]]

    platform="$os/$arch"
    [[ "$(jq --arg platform "$platform" 'index($platform)' <<<"$seen_platforms")" == null ]]
    seen_platforms="$(jq -c --arg platform "$platform" '. + [$platform]' <<<"$seen_platforms")"
    platform_tag="platform-$os-$arch-$publication_commit"
    platform_tags+=("$platform_tag")
    payload="$(mktemp -d)"
    cp "$binary" "$payload/plugin$suffix"
    (
      cd "$payload"
      oras push \
        --oci-layout "$workspace_root/$layout:$platform_tag" \
        --artifact-platform "$platform" \
        --artifact-type application/vnd.sproutos.template-plugin.v1 \
        --annotation "org.opencontainers.image.created=$created" \
        --annotation org.opencontainers.image.source=https://github.com/MySproutOS/Deployment-Templates \
        --annotation "org.opencontainers.image.revision=$publication_commit" \
        --annotation org.opencontainers.image.licenses=Apache-2.0 \
        --annotation "dev.sproutos.isolation-proof.kind=$kind" \
        "plugin$suffix:application/vnd.sproutos.template-plugin.executable.v1"
    )
    rm -rf "$payload"
    manifest_digest="$(oras resolve --oci-layout "$layout:$platform_tag")"
    platforms="$(jq -c \
      --arg platform "$platform" \
      --arg target "$target" \
      --arg manifest_digest "$manifest_digest" \
      --arg binary_digest "$expected_digest" \
      '. + [{platform:$platform,target:$target,manifestDigest:$manifest_digest,binaryDigest:$binary_digest}]' \
      <<<"$platforms")"
  done < <(find "$built_dir" -type f -name metadata.json | LC_ALL=C sort)

  actual_platforms="$(jq -c 'sort' <<<"$seen_platforms")"
  [[ "$actual_platforms" == "$expected_platforms" ]] || {
    echo "$kind has unexpected platforms: $actual_platforms" >&2
    exit 1
  }
  oras manifest index create \
    --oci-layout \
    --artifact-type application/vnd.sproutos.template-plugin.index.v1 \
    --annotation "org.opencontainers.image.created=$created" \
    --annotation org.opencontainers.image.source=https://github.com/MySproutOS/Deployment-Templates \
    --annotation "org.opencontainers.image.revision=$publication_commit" \
    --annotation org.opencontainers.image.licenses=Apache-2.0 \
    --annotation "dev.sproutos.isolation-proof.kind=$kind" \
    "$layout:$root_tag" \
    "${platform_tags[@]}"
  root_digest="$(oras resolve --oci-layout "$layout:$root_tag")"
  subjects="$(jq -c \
    --arg id "$kind" \
    --arg name "$repository" \
    --arg tag "$root_tag" \
    --arg digest "$root_digest" \
    --arg layout "oci/$kind" \
    --argjson platforms "$platforms" \
    '. + [{id:$id,name:$name,tag:$tag,digest:$digest,layout:$layout,platforms:($platforms|sort_by(.platform))}]' \
    <<<"$subjects")"
done

jq -Sn \
  --arg source_commit "$publication_commit" \
  --argjson subjects "$subjects" \
  '{schemaVersion:1,sourceCommit:$source_commit,workflow:".github/workflows/publish.yml",subjects:$subjects}' \
  >"$release_dir/subjects.json"
cp "$artifact_dir/source-lock.json" "$release_dir/source-lock.json"
(
  cd "$release_dir"
  if command -v sha256sum >/dev/null; then
    sha256sum source-lock.json subjects.json >SHA256SUMS
  else
    shasum -a 256 source-lock.json subjects.json >SHA256SUMS
  fi
)
