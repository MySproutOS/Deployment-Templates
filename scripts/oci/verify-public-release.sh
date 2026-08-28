#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <subjects.json> <expected-catalogue-dir>" >&2
  exit 2
fi

subjects_file="$1"
catalogue_dir="$2"
identity="https://github.com/${GITHUB_REPOSITORY}/.github/workflows/publish.yml@${GITHUB_REF}"
expected_platforms='["darwin/amd64","darwin/arm64","linux/amd64","linux/arm64","windows/amd64"]'

while IFS= read -r subject; do
  kind="$(jq -r '.kind' <<<"$subject")"
  name="$(jq -r '.name' <<<"$subject")"
  digest="$(jq -r '.digest' <<<"$subject")"
  reference="$name@$digest"

  # This job never logs into GHCR. Success therefore proves anonymous public access.
  manifest="$(oras manifest fetch "$reference")"
  if [[ "$kind" == plugin ]]; then
    [[ "$(jq -r '.mediaType' <<<"$manifest")" == application/vnd.oci.image.index.v1+json ]]
    [[ "$(jq -r '.artifactType' <<<"$manifest")" == application/vnd.sproutos.template-plugin.index.v1 ]]
    platforms="$(jq -c '[.manifests[].platform | "\(.os)/\(.architecture)"] | sort' <<<"$manifest")"
    [[ "$platforms" == "$expected_platforms" ]] || {
      echo "$reference has unexpected platforms: $platforms" >&2
      exit 1
    }
  else
    [[ "$(jq -r '.artifactType' <<<"$manifest")" == application/vnd.sproutos.deployment-catalogue.v1 ]]
    pulled="$(mktemp -d)"
    oras pull "$reference" --output "$pulled"
    cmp "$pulled/catalogue.json" "$catalogue_dir/catalogue.json"
    cmp "$pulled/provenance.json" "$catalogue_dir/provenance.json"
    cmp "$pulled/plugin-lock.json" "$catalogue_dir/plugin-lock.json"
    rm -rf "$pulled"
  fi

  cosign verify \
    --certificate-identity "$identity" \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    "$reference" >/dev/null
  gh attestation verify "oci://$reference" \
    --bundle-from-oci \
    --repo "$GITHUB_REPOSITORY" \
    --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/publish.yml" \
    --source-digest "$GITHUB_SHA" \
    --source-ref "$GITHUB_REF" \
    --deny-self-hosted-runners >/dev/null
done < <(jq -c '.subjects[]' "$subjects_file")
