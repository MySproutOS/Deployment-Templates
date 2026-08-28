#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <subjects.json> <expected-source-commit>" >&2
  exit 2
fi

subjects_file="$1"
source_commit="$2"
[[ "$source_commit" =~ ^[0-9a-f]{40}$ ]]

jq -e --arg source_commit "$source_commit" '
  .schemaVersion == 1 and
  .sourceCommit == $source_commit and
  .workflow == ".github/workflows/publish.yml" and
  (.subjects | length) == 3 and
  ([.subjects[].id] | sort) == ["fork-timeout", "stdout-flood", "success"] and
  ([.subjects[].id] | unique | length) == 3 and
  ([.subjects[].name] | unique | length) == 3 and
  ([.subjects[].digest] | unique | length) == 3 and
  all(.subjects[];
    .name == ("ghcr.io/mysproutos/isolation-proof-" + .id) and
    .tag == ("sha-" + $source_commit) and
    .layout == ("oci/" + .id) and
    (.digest | test("^sha256:[0-9a-f]{64}$")) and
    (.platforms | length) == 5 and
    ([.platforms[].platform] | sort) ==
      ["darwin/amd64", "darwin/arm64", "linux/amd64", "linux/arm64", "windows/amd64"]
  )
' "$subjects_file" >/dev/null
