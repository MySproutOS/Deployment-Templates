#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <layout> <name> <tag> <expected-digest>" >&2
  exit 2
fi

layout="$1"
name="$2"
tag="$3"
expected="$4"
[[ "$name" =~ ^ghcr\.io/[a-z0-9-]+(/[a-z0-9-]+)+$ ]] || exit 1
[[ "$tag" =~ ^sha-[0-9a-f]{40}$ ]] || exit 1
[[ "$expected" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 1

if remote="$(oras resolve "$name:$tag" 2>/dev/null)"; then
  [[ "$remote" == "$expected" ]] || {
    echo "Refusing to move $name:$tag from $remote to $expected" >&2
    exit 1
  }
else
  oras cp --recursive --from-oci-layout "$layout:$tag" "$name:$tag"
fi

remote="$(oras resolve "$name:$tag")"
[[ "$remote" == "$expected" ]] || {
  echo "Registry returned $remote, expected $expected" >&2
  exit 1
}

reference="$name@$expected"
# Avoid adding duplicate referrers when a failed first publication is rerun after making GHCR public.
if ! cosign verify \
  --certificate-identity "$COSIGN_IDENTITY" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "$reference" >/dev/null 2>&1; then
  cosign sign --yes "$reference"
fi
