#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <subjects.json>" >&2
  exit 2
fi

subjects_file="$1"
expected_platforms='["darwin/amd64","darwin/arm64","linux/amd64","linux/arm64","windows/amd64"]'
identity="https://github.com/${GITHUB_REPOSITORY}/.github/workflows/publish.yml@${GITHUB_REF}"
[[ "$(jq -r '.sourceCommit' "$subjects_file")" == "$GITHUB_SHA" ]]
[[ "$(jq -r '.workflow' "$subjects_file")" == .github/workflows/publish.yml ]]

while IFS= read -r subject; do
  kind="$(jq -r '.id' <<<"$subject")"
  name="$(jq -r '.name' <<<"$subject")"
  digest="$(jq -r '.digest' <<<"$subject")"
  reference="$name@$digest"
  [[ "$kind" == success || "$kind" == stdout-flood || "$kind" == fork-timeout ]]
  [[ "$name" == "ghcr.io/mysproutos/isolation-proof-$kind" ]]
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]

  # No registry login occurs in this job. Every fetch therefore proves public visibility.
  index="$(oras manifest fetch "$reference")"
  [[ "$(jq -r '.schemaVersion' <<<"$index")" == 2 ]]
  [[ "$(jq -r '.artifactType' <<<"$index")" == application/vnd.sproutos.template-plugin.index.v1 ]]
  [[ "$(jq -r '.annotations["org.opencontainers.image.source"]' <<<"$index")" == https://github.com/MySproutOS/Deployment-Templates ]]
  [[ "$(jq -r '.annotations["org.opencontainers.image.licenses"]' <<<"$index")" == Apache-2.0 ]]
  [[ "$(jq -r '.annotations["org.opencontainers.image.revision"]' <<<"$index")" == "$GITHUB_SHA" ]]
  [[ "$(jq -r '.annotations["dev.sproutos.isolation-proof.kind"]' <<<"$index")" == "$kind" ]]
  platforms="$(jq -c '[.manifests[].platform | "\(.os)/\(.architecture)"] | sort' <<<"$index")"
  [[ "$platforms" == "$expected_platforms" ]]
  [[ "$(jq '.manifests | length' <<<"$index")" == 5 ]]

  while IFS= read -r descriptor; do
    platform="$(jq -r '.platform | "\(.os)/\(.architecture)"' <<<"$descriptor")"
    manifest_digest="$(jq -r '.digest' <<<"$descriptor")"
    [[ "$(jq -r '.artifactType' <<<"$descriptor")" == application/vnd.sproutos.template-plugin.v1 ]]
    expected_manifest="$(jq -r --arg platform "$platform" '.platforms[] | select(.platform == $platform) | .manifestDigest' <<<"$subject")"
    [[ "$manifest_digest" == "$expected_manifest" ]]
    manifest="$(oras manifest fetch "$name@$manifest_digest")"
    [[ "$(jq -r '.schemaVersion' <<<"$manifest")" == 2 ]]
    [[ "$(jq -r '.artifactType' <<<"$manifest")" == application/vnd.sproutos.template-plugin.v1 ]]
    [[ "$(jq -r '.annotations["org.opencontainers.image.source"]' <<<"$manifest")" == https://github.com/MySproutOS/Deployment-Templates ]]
    [[ "$(jq -r '.annotations["org.opencontainers.image.licenses"]' <<<"$manifest")" == Apache-2.0 ]]
    [[ "$(jq -r '.annotations["org.opencontainers.image.revision"]' <<<"$manifest")" == "$GITHUB_SHA" ]]
    [[ "$(jq -r '.annotations["dev.sproutos.isolation-proof.kind"]' <<<"$manifest")" == "$kind" ]]
    [[ "$(jq '.layers | length' <<<"$manifest")" == 1 ]]
    layer="$(jq -c '.layers[0]' <<<"$manifest")"
    [[ "$(jq -r '.mediaType' <<<"$layer")" == application/vnd.sproutos.template-plugin.executable.v1 ]]
    [[ "$(jq -r '.size' <<<"$layer")" -le 67108864 ]]
    expected_title=plugin
    [[ "$platform" == windows/amd64 ]] && expected_title=plugin.exe
    [[ "$(jq -r '.annotations["org.opencontainers.image.title"]' <<<"$layer")" == "$expected_title" ]]
    layer_digest="$(jq -r '.digest' <<<"$layer")"
    expected_binary="$(jq -r --arg platform "$platform" '.platforms[] | select(.platform == $platform) | .binaryDigest' <<<"$subject")"
    [[ "$layer_digest" == "$expected_binary" ]]
    pulled="$(mktemp)"
    oras blob fetch --output "$pulled" "$name@$layer_digest"
    if command -v sha256sum >/dev/null; then
      actual_binary="sha256:$(sha256sum "$pulled" | awk '{print $1}')"
    else
      actual_binary="sha256:$(shasum -a 256 "$pulled" | awk '{print $1}')"
    fi
    rm -f "$pulled"
    [[ "$actual_binary" == "$expected_binary" ]]
  done < <(jq -c '.manifests[]' <<<"$index")

  cosign verify \
    --certificate-identity "$identity" \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    --certificate-github-workflow-sha "$GITHUB_SHA" \
    "$reference" >/dev/null
  gh attestation verify "oci://$reference" \
    --bundle-from-oci \
    --repo "$GITHUB_REPOSITORY" \
    --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/publish.yml" \
    --source-digest "$GITHUB_SHA" \
    --source-ref "$GITHUB_REF" \
    --deny-self-hosted-runners >/dev/null
done < <(jq -c '.subjects[]' "$subjects_file")
