#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "usage: $0 <built-plugins-dir> <release-dir> <registry-base> <source-url> <commit> <catalogue-command>" >&2
  exit 2
fi

built_dir="$1"
release_dir="$2"
registry_base="${3%/}"
source_url="$4"
commit="$5"
catalogue_command="$6"

[[ "$registry_base" =~ ^ghcr\.io/[a-z0-9-]+$ ]] || {
  echo "Registry base must be a lowercase GHCR organization path: $registry_base" >&2
  exit 1
}
[[ "$commit" =~ ^[0-9a-f]{40}$ ]] || {
  echo "Commit must be a full lowercase Git SHA." >&2
  exit 1
}
command -v oras >/dev/null
command -v jq >/dev/null
workspace_root="$(pwd -P)"
# ORAS otherwise injects the wall-clock time and changes every manifest digest on rerun.
# Epoch is a reproducibility sentinel, not a claim about the upstream application's age.
created=1970-01-01T00:00:00Z

[[ "$built_dir" == dist/plugins && "$release_dir" == release ]] || {
  echo "This release assembler only operates on its fixed CI staging paths." >&2
  exit 1
}

rm -rf "$release_dir"
mkdir -p "$release_dir/oci" catalogue
plugin_lock='{"schemaVersion":1,"plugins":{}}'
subjects='[]'

metadata_files=()
while IFS= read -r metadata; do
  metadata_files+=("$metadata")
done < <(find "$built_dir" -type f -name metadata.json | LC_ALL=C sort)
[[ ${#metadata_files[@]} -gt 0 ]] || {
  echo "No built plugin metadata found under $built_dir" >&2
  exit 1
}

plugin_ids=()
while IFS= read -r id; do
  plugin_ids+=("$id")
done < <(jq -r '.id' "${metadata_files[@]}" | LC_ALL=C sort -u)
for id in "${plugin_ids[@]}"; do
  [[ "$id" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] || exit 1
  repository="$registry_base/$id-plugin"
  layout="$release_dir/oci/$id"
  index_tag="sha-$commit"
  platform_tags=()
  seen_platforms='[]'

  for metadata in "${metadata_files[@]}"; do
    [[ "$(jq -r '.id' "$metadata")" == "$id" ]] || continue
    target="$(jq -r '.target' "$metadata")"
    os="$(jq -r '.os' "$metadata")"
    arch="$(jq -r '.arch' "$metadata")"
    suffix="$(jq -r '.suffix' "$metadata")"
    expected="$(jq -r '.binaryDigest' "$metadata")"
    for value in "$target" "$os" "$arch"; do
      [[ "$value" =~ ^[a-zA-Z0-9_.-]+$ ]] || exit 1
    done
    binary="$(dirname "$metadata")/plugin${suffix}"
    actual="sha256:$(sha256sum "$binary" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]] || {
      echo "Binary digest mismatch for $id/$target" >&2
      exit 1
    }

    platform="$os/$arch"
    if jq -e --arg platform "$platform" 'index($platform) != null' <<<"$seen_platforms" >/dev/null; then
      echo "Duplicate platform $platform for $id" >&2
      exit 1
    fi
    seen_platforms="$(jq -c --arg platform "$platform" '. + [$platform]' <<<"$seen_platforms")"
    platform_tag="platform-${os}-${arch}-$commit"
    platform_tags+=("$platform_tag")
    payload="$(mktemp -d)"
    cp "$binary" "$payload/plugin${suffix}"
    (
      cd "$payload"
      oras push \
        --oci-layout "$workspace_root/$layout:$platform_tag" \
        --artifact-platform "$platform" \
        --artifact-type application/vnd.sproutos.template-plugin.v1 \
        --annotation "org.opencontainers.image.created=$created" \
        --annotation "org.opencontainers.image.source=$source_url" \
        --annotation "org.opencontainers.image.title=$id template plugin" \
        --annotation "org.opencontainers.image.description=Deterministic SproutOS deployment-template plugin for $id" \
        --annotation org.opencontainers.image.licenses=Apache-2.0 \
        "plugin${suffix}:application/vnd.sproutos.template-plugin.executable.v1"
    )
    rm -rf "$payload"
  done

  [[ "$(jq 'length' <<<"$seen_platforms")" -eq 5 ]] || {
    echo "$id must have exactly the five supported platforms; got $seen_platforms" >&2
    exit 1
  }
  oras manifest index create \
    --oci-layout \
    --artifact-type application/vnd.sproutos.template-plugin.index.v1 \
    --annotation "org.opencontainers.image.created=$created" \
    --annotation "org.opencontainers.image.source=$source_url" \
    --annotation "org.opencontainers.image.title=$id template plugin" \
    --annotation "org.opencontainers.image.description=Cross-platform SproutOS deployment-template plugin for $id" \
    --annotation org.opencontainers.image.licenses=Apache-2.0 \
    "$layout:$index_tag" \
    "${platform_tags[@]}"
  digest="$(oras resolve --oci-layout "$layout:$index_tag")"
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 1

  plugin_lock="$(jq -c \
    --arg id "$id" \
    --arg artifact "$repository@$digest" \
    '.plugins[$id] = {artifact: $artifact}' <<<"$plugin_lock")"
  subjects="$(jq -c \
    --arg kind plugin \
    --arg id "$id" \
    --arg name "$repository" \
    --arg tag "$index_tag" \
    --arg digest "$digest" \
    --arg layout "oci/$id" \
    '. + [{kind:$kind,id:$id,name:$name,tag:$tag,digest:$digest,layout:$layout}]' <<<"$subjects")"
done

jq -S . <<<"$plugin_lock" >catalogue/plugin-lock.json

# The command is a fixed repository-owned binary name, never metadata-controlled input.
read -r -a generator <<<"$catalogue_command"
"${generator[@]}" \
  --plugin-lock catalogue/plugin-lock.json \
  --output catalogue/catalogue.json \
  --provenance-output catalogue/provenance.json \
  --source-repository "${GITHUB_REPOSITORY:?}" \
  --source-workflow .github/workflows/publish.yml \
  --source-ref "${GITHUB_REF:?}" \
  --source-commit "$commit"

catalogue_repository="$registry_base/deployment-catalogue"
catalogue_layout="$release_dir/oci/catalogue"
catalogue_tag="sha-$commit"
catalogue_payload="$(mktemp -d)"
cp catalogue/catalogue.json catalogue/provenance.json catalogue/plugin-lock.json "$catalogue_payload/"
(
  cd "$catalogue_payload"
  oras push \
    --oci-layout "$workspace_root/$catalogue_layout:$catalogue_tag" \
    --artifact-type application/vnd.sproutos.deployment-catalogue.v1 \
    --annotation "org.opencontainers.image.created=$created" \
    --annotation "org.opencontainers.image.source=$source_url" \
    --annotation "org.opencontainers.image.title=SproutOS deployment catalogue" \
    --annotation "org.opencontainers.image.description=Signed SproutOS deployment-template catalogue" \
    --annotation org.opencontainers.image.licenses=Apache-2.0 \
    "catalogue.json:application/vnd.sproutos.deployment-catalogue.v1+json" \
    "provenance.json:application/vnd.sproutos.deployment-catalogue.provenance.v1+json" \
    "plugin-lock.json:application/vnd.sproutos.template-plugin-lock.v1+json"
)
rm -rf "$catalogue_payload"
catalogue_digest="$(oras resolve --oci-layout "$catalogue_layout:$catalogue_tag")"
[[ "$catalogue_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 1
subjects="$(jq -c \
  --arg kind catalogue \
  --arg id catalogue \
  --arg name "$catalogue_repository" \
  --arg tag "$catalogue_tag" \
  --arg digest "$catalogue_digest" \
  --arg layout oci/catalogue \
  '. + [{kind:$kind,id:$id,name:$name,tag:$tag,digest:$digest,layout:$layout}]' <<<"$subjects")"

mkdir -p "$release_dir/catalogue"
cp catalogue/catalogue.json catalogue/provenance.json catalogue/plugin-lock.json "$release_dir/catalogue/"
jq -Sn \
  --arg commit "$commit" \
  --argjson subjects "$subjects" \
  '{schemaVersion:1,sourceCommit:$commit,subjects:($subjects|sort_by(.kind,.id))}' \
  >"$release_dir/subjects.json"

(
  cd "$release_dir/catalogue"
  sha256sum catalogue.json provenance.json plugin-lock.json >SHA256SUMS
)
