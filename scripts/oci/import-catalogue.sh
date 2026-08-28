#!/usr/bin/env bash
set -euo pipefail
umask 077

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <subjects.json> <catalogue-dir>" >&2
  exit 2
fi

subjects_file="$1"
catalogue_dir="$2"
repository=MySproutOS/Deployment-Templates
ref=refs/heads/main
workflow_ref="$repository/.github/workflows/publish.yml@$ref"
catalogue_repository=ghcr.io/mysproutos/deployment-catalogue
import_url=https://api.sproutos.me/v1/deploy/catalogue/import

for command in curl jq sha256sum; do
  command -v "$command" >/dev/null || {
    echo "$command is required to request a catalogue import." >&2
    exit 1
  }
done

[[ "${GITHUB_REPOSITORY:-}" == "$repository" ]] || {
  echo "Catalogue imports are restricted to $repository." >&2
  exit 1
}
[[ "${GITHUB_REF:-}" == "$ref" ]] || {
  echo "Catalogue imports are restricted to $ref." >&2
  exit 1
}
[[ "${GITHUB_WORKFLOW_REF:-}" == "$workflow_ref" ]] || {
  echo "Catalogue imports are restricted to $workflow_ref." >&2
  exit 1
}
[[ "${GITHUB_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || {
  echo "GITHUB_SHA must be a full lowercase commit SHA." >&2
  exit 1
}
[[ -n "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:-}" ]] || {
  echo "GitHub OIDC request token is unavailable; grant only id-token: write to this job." >&2
  exit 1
}
[[ "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" =~ ^https://[^[:space:]]+$ ]] || {
  echo "GitHub OIDC request URL is unavailable or is not HTTPS." >&2
  exit 1
}

for path in \
  "$subjects_file" \
  "$catalogue_dir/catalogue.json" \
  "$catalogue_dir/provenance.json" \
  "$catalogue_dir/plugin-lock.json" \
  "$catalogue_dir/SHA256SUMS"; do
  [[ -f "$path" ]] || {
    echo "Missing verified release input: $path" >&2
    exit 1
  }
done

# The downloaded artifact is the same immutable run artifact verified anonymously from GHCR by the
# preceding job. Recheck its local byte contract before requesting any control-plane work.
checksum_names="$(awk '{print $2}' "$catalogue_dir/SHA256SUMS" | LC_ALL=C sort | paste -sd, -)"
[[ "$checksum_names" == 'catalogue.json,plugin-lock.json,provenance.json' ]] || {
  echo "Catalogue checksums do not name the exact three imported documents." >&2
  exit 1
}
(
  cd "$catalogue_dir"
  sha256sum --check --strict SHA256SUMS >/dev/null
)
catalogue_file_digest="sha256:$(sha256sum "$catalogue_dir/catalogue.json" | awk '{print $1}')"

jq -e \
  --arg repository "$repository" \
  --arg ref "$ref" \
  --arg sha "$GITHUB_SHA" \
  --arg digest "$catalogue_file_digest" '
    .schema_version == 1 and
    .repository == $repository and
    .workflow == ".github/workflows/publish.yml" and
    .ref == $ref and
    .source_commit == $sha and
    .subject == {
      kind: "catalogue",
      name: "catalogue/catalogue.json",
      digest: $digest
    }
  ' "$catalogue_dir/provenance.json" >/dev/null || {
  echo "Catalogue provenance does not match the trusted repository, workflow, ref, SHA, and bytes." >&2
  exit 1
}

jq -e --arg sha "$GITHUB_SHA" \
  '.schema_version == 1 and .generated_from_commit == $sha' \
  "$catalogue_dir/catalogue.json" >/dev/null || {
  echo "Catalogue bytes do not identify the workflow source SHA." >&2
  exit 1
}

jq -e \
  --arg repository "$catalogue_repository" \
  --arg sha "$GITHUB_SHA" '
    .schemaVersion == 1 and
    .sourceCommit == $sha and
    (.subjects | type == "array") and
    ([.subjects[] | select(.kind == "catalogue")] | length) == 1 and
    ([.subjects[] | select(
      .kind == "catalogue" and
      .id == "catalogue" and
      .name == $repository and
      .tag == ("sha-" + $sha) and
      .layout == "oci/catalogue" and
      (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$"))
    )] | length) == 1
  ' "$subjects_file" >/dev/null || {
  echo "Release subjects do not contain exactly one catalogue pinned to the workflow source SHA." >&2
  exit 1
}

oci_digest="$(jq -er '.subjects[] | select(.kind == "catalogue") | .digest' "$subjects_file")"

temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

# Mint once and reuse the same short-lived token for every transport retry. The API's idempotency
# key includes this run ID and digest, so a timeout after acceptance cannot queue divergent work.
oidc_response="$temporary/oidc.json"
curl --silent --show-error --fail \
  --proto '=https' --tlsv1.2 \
  --connect-timeout 10 --max-time 20 \
  --retry 4 --retry-delay 2 --retry-all-errors \
  --header "Authorization: Bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
  --output "$oidc_response" \
  "${ACTIONS_ID_TOKEN_REQUEST_URL}&audience=sproutos"
oidc_token="$(jq -er '.value | select(type == "string" and length > 0)' "$oidc_response")" || {
  echo "GitHub OIDC response carried no token." >&2
  exit 1
}

request="$temporary/request.json"
response="$temporary/response.json"
jq -n --arg token "$oidc_token" --arg digest "$oci_digest" \
  '{oidc_token:$token,oci_digest:$digest}' >"$request"
unset oidc_token

for attempt in 1 2 3 4 5; do
  status=000
  if status="$(curl --silent --show-error \
    --proto '=https' --tlsv1.2 \
    --connect-timeout 10 --max-time 30 \
    --request POST \
    --header 'Content-Type: application/json' \
    --data-binary "@$request" \
    --output "$response" \
    --write-out '%{http_code}' \
    "$import_url")"; then
    if [[ "$status" == 202 ]]; then
      jq -e --arg digest "$oci_digest" '
        (.job_id | type == "string" and length > 0) and .oci_digest == $digest
      ' "$response" >/dev/null || {
        echo "SproutOS accepted the request but did not confirm the exact OCI digest." >&2
        exit 1
      }
      job_id="$(jq -r '.job_id' "$response")"
      echo "Queued verified catalogue import job $job_id for $oci_digest."
      exit 0
    fi
    case "$status" in
      408|429|500|502|503|504) ;;
      *)
        message="$(jq -r '.message // empty' "$response" 2>/dev/null || true)"
        echo "SproutOS catalogue import was refused with HTTP $status${message:+: $message}" >&2
        exit 1
        ;;
    esac
  fi

  if [[ "$attempt" -eq 5 ]]; then
    echo "SproutOS catalogue import did not succeed after $attempt attempts (last HTTP status $status)." >&2
    exit 1
  fi
  sleep "$((attempt * 2))"
done
