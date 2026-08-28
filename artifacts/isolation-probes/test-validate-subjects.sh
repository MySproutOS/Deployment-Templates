#!/usr/bin/env bash
set -euo pipefail

artifact_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
validator="$artifact_dir/validate-subjects.sh"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT
commit=ca6c6168d49522d2deb9433e73fba3bc6f65f74a
platforms='[{"platform":"darwin/amd64"},{"platform":"darwin/arm64"},{"platform":"linux/amd64"},{"platform":"linux/arm64"},{"platform":"windows/amd64"}]'

jq -n --arg commit "$commit" --argjson platforms "$platforms" '
  {schemaVersion:1,sourceCommit:$commit,workflow:".github/workflows/publish.yml",subjects:
    ["success","stdout-flood","fork-timeout"] | map({
      id:.,
      name:("ghcr.io/mysproutos/isolation-proof-" + .),
      tag:("sha-" + $commit),
      digest:("sha256:" + (if . == "success" then "1" elif . == "stdout-flood" then "2" else "3" end) * 64),
      layout:("oci/" + .),
      platforms:$platforms
    })}
' >"$temporary/valid.json"
"$validator" "$temporary/valid.json" "$commit"

for mutation in missing duplicate unknown wrong-name duplicate-digest; do
  case "$mutation" in
    missing) filter='.subjects |= .[0:2]' ;;
    duplicate) filter='.subjects[2] = .subjects[0]' ;;
    unknown) filter='.subjects[2].id = "unknown"' ;;
    wrong-name) filter='.subjects[0].name = "ghcr.io/example/wrong"' ;;
    duplicate-digest) filter='.subjects[2].digest = .subjects[0].digest' ;;
  esac
  jq "$filter" "$temporary/valid.json" >"$temporary/$mutation.json"
  if "$validator" "$temporary/$mutation.json" "$commit"; then
    echo "Invalid proof subjects passed validation: $mutation" >&2
    exit 1
  fi
done

echo 'proof subject validation tests passed'
