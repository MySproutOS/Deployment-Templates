#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
importer="$repository_root/scripts/oci/import-catalogue.sh"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

export GITHUB_REPOSITORY=MySproutOS/Deployment-Templates
export GITHUB_REF=refs/heads/main
export GITHUB_WORKFLOW_REF=MySproutOS/Deployment-Templates/.github/workflows/publish.yml@refs/heads/main
export GITHUB_SHA=ca6c6168d49522d2deb9433e73fba3bc6f65f74a
export ACTIONS_ID_TOKEN_REQUEST_URL='https://vstoken.actions.githubusercontent.test/oidc?run=1'
export ACTIONS_ID_TOKEN_REQUEST_TOKEN=request-token
export MOCK_CALLS="$temporary/calls"
export MOCK_ATTEMPTS="$temporary/attempts"

mkdir -p "$temporary/bin" "$temporary/release/catalogue"
cat >"$temporary/bin/curl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >>"$MOCK_CALLS"
output=''
write_out=''
url="${!#}"
request=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    --write-out) write_out="$2"; shift 2 ;;
    --data-binary) request="${2#@}"; shift 2 ;;
    *) shift ;;
  esac
done

if [[ "$url" == *audience=sproutos ]]; then
  printf '{"value":"header.payload.signature"}\n' >"$output"
  exit 0
fi

[[ "$url" == https://api.sproutos.me/v1/deploy/catalogue/import ]]
jq -e '
  .oidc_token == "header.payload.signature" and
  .oci_digest == "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
' "$request" >/dev/null
count=0
[[ ! -f "$MOCK_ATTEMPTS" ]] || count="$(cat "$MOCK_ATTEMPTS")"
count=$((count + 1))
printf '%s\n' "$count" >"$MOCK_ATTEMPTS"
if [[ "$count" -lt 3 ]]; then
  printf '{"message":"retry"}\n' >"$output"
  printf '503'
else
  printf '{"job_id":"same-idempotent-job","oci_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}\n' >"$output"
  printf '202'
fi
[[ "$write_out" == '%{http_code}' ]]
MOCK
cat >"$temporary/bin/sleep" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
:
MOCK
chmod +x "$temporary/bin/curl" "$temporary/bin/sleep"
export PATH="$temporary/bin:$PATH"

cat >"$temporary/release/catalogue/catalogue.json" <<JSON
{"schema_version":1,"generated_from_commit":"$GITHUB_SHA","apps":[]}
JSON
catalogue_digest="sha256:$(sha256sum "$temporary/release/catalogue/catalogue.json" | awk '{print $1}')"
cat >"$temporary/release/catalogue/provenance.json" <<JSON
{"schema_version":1,"repository":"$GITHUB_REPOSITORY","workflow":".github/workflows/publish.yml","ref":"$GITHUB_REF","source_commit":"$GITHUB_SHA","subject":{"kind":"catalogue","name":"catalogue/catalogue.json","digest":"$catalogue_digest"},"materials":[]}
JSON
printf '{}\n' >"$temporary/release/catalogue/plugin-lock.json"
(
  cd "$temporary/release/catalogue"
  sha256sum catalogue.json provenance.json plugin-lock.json >SHA256SUMS
)
cat >"$temporary/release/subjects.json" <<JSON
{"schemaVersion":1,"sourceCommit":"$GITHUB_SHA","subjects":[{"kind":"catalogue","id":"catalogue","name":"ghcr.io/mysproutos/deployment-catalogue","tag":"sha-$GITHUB_SHA","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","layout":"oci/catalogue"}]}
JSON

output="$($importer "$temporary/release/subjects.json" "$temporary/release/catalogue")"
[[ "$output" == *'same-idempotent-job'* ]]
[[ "$(cat "$MOCK_ATTEMPTS")" == 3 ]]
[[ "$(grep -c 'audience=sproutos' "$MOCK_CALLS")" == 1 ]]
[[ "$(grep -c 'https://api.sproutos.me/v1/deploy/catalogue/import' "$MOCK_CALLS")" == 3 ]]
if grep -Fq 'header.payload.signature' "$MOCK_CALLS"; then
  echo 'OIDC token was passed on the command line.' >&2
  exit 1
fi

printf '' >"$MOCK_CALLS"
jq '.sourceCommit = "0000000000000000000000000000000000000000"' \
  "$temporary/release/subjects.json" >"$temporary/release/wrong-subjects.json"
if "$importer" "$temporary/release/wrong-subjects.json" "$temporary/release/catalogue" >/dev/null 2>&1; then
  echo 'Importer accepted subjects from another source SHA.' >&2
  exit 1
fi
[[ ! -s "$MOCK_CALLS" ]]

printf '' >"$MOCK_CALLS"
jq '.source_commit = "0000000000000000000000000000000000000000"' \
  "$temporary/release/catalogue/provenance.json" >"$temporary/release/catalogue/provenance.next"
mv "$temporary/release/catalogue/provenance.next" "$temporary/release/catalogue/provenance.json"
(
  cd "$temporary/release/catalogue"
  sha256sum catalogue.json provenance.json plugin-lock.json >SHA256SUMS
)
if "$importer" "$temporary/release/subjects.json" "$temporary/release/catalogue" >/dev/null 2>&1; then
  echo 'Importer accepted provenance from another source SHA.' >&2
  exit 1
fi
[[ ! -s "$MOCK_CALLS" ]]

echo 'catalogue import delivery tests passed'
