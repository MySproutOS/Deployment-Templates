#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repository_root/.github/workflows/publish.yml"
checkout='actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1'
publisher='scripts/oci/publish-immutable-release.sh'

assert_release_job() {
  local job="$1"
  local block
  local checkout_line
  local publisher_line

  block="$(awk -v header="  $job:" '
    $0 == header { inside = 1 }
    inside && $0 != header && $0 ~ /^  [a-zA-Z0-9_-]+:$/ { exit }
    inside { print }
  ' "$workflow")"

  [[ -n "$block" ]] || {
    echo "Missing publish workflow job: $job" >&2
    exit 1
  }

  checkout_line="$(grep -nF "$checkout" <<<"$block" | cut -d: -f1)"
  publisher_line="$(grep -nF "$publisher" <<<"$block" | cut -d: -f1)"
  [[ -n "$checkout_line" && -n "$publisher_line" ]] || {
    echo "$job must check out the publisher script before invoking it." >&2
    exit 1
  }
  (( checkout_line < publisher_line )) || {
    echo "$job invokes the publisher before checkout." >&2
    exit 1
  }
  grep -A2 -F "$checkout" <<<"$block" | grep -Fq 'persist-credentials: false' || {
    echo "$job checkout must disable persisted credentials." >&2
    exit 1
  }
}

assert_release_job release
assert_release_job isolation-proof-release

echo 'publish workflow checkout tests passed'
