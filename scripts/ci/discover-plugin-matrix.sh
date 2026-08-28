#!/usr/bin/env bash
set -euo pipefail

# Discover plugin packages from Cargo rather than coupling CI to a source-directory layout.
# A plugin package must have one binary target and be named sprout-template-<stable-id>.

metadata="$(cargo metadata --locked --no-deps --format-version 1)"
plugins="$({
  jq -r '
    .packages[]
    | select((.name | startswith("sprout-template-")) and (.manifest_path | contains("/plugins/")))
    | [
        .name,
        (.name | sub("^sprout-template-"; "")),
        ([.targets[] | select(.kind | index("bin")) | .name] | join(","))
      ]
    | @tsv
  ' <<<"$metadata"
} | LC_ALL=C sort)"

if [[ -z "$plugins" ]]; then
  echo "No Cargo packages named sprout-template-<id> were found." >&2
  exit 1
fi

platforms='[
  {"runner":"ubuntu-24.04",     "target":"x86_64-unknown-linux-musl",  "os":"linux",   "arch":"amd64", "suffix":""},
  {"runner":"ubuntu-24.04-arm", "target":"aarch64-unknown-linux-musl", "os":"linux",   "arch":"arm64", "suffix":""},
  {"runner":"macos-15-intel",   "target":"x86_64-apple-darwin",       "os":"darwin",  "arch":"amd64", "suffix":""},
  {"runner":"macos-15",         "target":"aarch64-apple-darwin",      "os":"darwin",  "arch":"arm64", "suffix":""},
  {"runner":"windows-2025",     "target":"x86_64-pc-windows-msvc",   "os":"windows", "arch":"amd64", "suffix":".exe"}
]'

matrix='[]'
while IFS=$'\t' read -r package id binaries; do
  [[ "$id" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] || {
    echo "Invalid stable plugin id derived from package $package: $id" >&2
    exit 1
  }
  if [[ "$binaries" == *,* || -z "$binaries" ]]; then
    echo "$package must expose exactly one binary target; found: ${binaries:-none}" >&2
    exit 1
  fi

  expanded="$(jq -c \
    --arg package "$package" \
    --arg id "$id" \
    --arg bin "$binaries" \
    'map(. + {package: $package, id: $id, bin: $bin})' <<<"$platforms")"
  matrix="$(jq -c --argjson expanded "$expanded" '. + $expanded' <<<"$matrix")"
done <<<"$plugins"

jq -cn --argjson include "$matrix" '{include: $include}'
