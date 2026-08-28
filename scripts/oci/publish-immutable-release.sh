#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 4 ]]; then
  echo "usage: $0 <tag> <title> <notes> <asset>..." >&2
  exit 2
fi

tag="$1"
title="$2"
notes="$3"
shift 3
assets=("$@")
repository="${GITHUB_REPOSITORY:?}"
source_commit="${GITHUB_SHA:?}"
: "${GH_TOKEN:?}"

[[ "$tag" =~ ^[A-Za-z0-9._-]+$ ]]
[[ "$source_commit" =~ ^[0-9a-f]{40}$ ]]

expected_names='[]'
for asset in "${assets[@]}"; do
  [[ -f "$asset" ]]
  name="$(basename "$asset")"
  [[ "$name" == "$asset" || "$asset" == */"$name" ]]
  if jq -e --arg name "$name" 'index($name) != null' <<<"$expected_names" >/dev/null; then
    echo "Duplicate release asset name: $name" >&2
    exit 1
  fi
  expected_names="$(jq -ce --arg name "$name" '. + [$name] | sort' <<<"$expected_names")" || exit 1
done

temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT
release_json="$temporary/release.json"

api_get() {
  local endpoint="$1"
  local output="$2"
  local error="$temporary/api-error"
  if gh api "$endpoint" >"$output" 2>"$error"; then
    return 0
  fi
  if grep -q 'HTTP 404' "$error"; then
    return 4
  fi
  cat "$error" >&2
  return 1
}

resolve_tag() {
  local object_json="$temporary/tag-object.json"
  local annotated_json="$temporary/annotated-tag.json"
  api_get "repos/$repository/git/ref/tags/$tag" "$object_json" || {
    result=$?
    return "$result"
  }
  for _ in 1 2 3 4 5 6 7 8; do
    object_type="$(jq -er '.object.type' "$object_json")" || return 1
    object_sha="$(jq -er '.object.sha' "$object_json")" || return 1
    [[ "$object_sha" =~ ^[0-9a-f]{40}$ ]] || return 1
    if [[ "$object_type" == commit ]]; then
      printf '%s\n' "$object_sha"
      return 0
    fi
    [[ "$object_type" == tag ]] || return 1
    api_get "repos/$repository/git/tags/$object_sha" "$annotated_json" || return 1
    cp "$annotated_json" "$object_json" || return 1
  done
  echo "Release tag annotation chain is too deep." >&2
  return 1
}

discover_release() {
  local pages="$temporary/releases.json"
  local matches="$temporary/matching-releases.json"
  local error="$temporary/releases-error"
  if ! gh api --paginate --slurp "repos/$repository/releases?per_page=100" >"$pages" 2>"$error"; then
    cat "$error" >&2
    return 1
  fi
  jq --arg tag "$tag" '[.[][] | select(.tag_name == $tag)]' "$pages" >"$matches" || {
    echo "Release listing was not the expected paginated JSON shape." >&2
    return 1
  }
  count="$(jq -er 'length' "$matches")" || return 1
  [[ "$count" =~ ^[0-9]+$ ]] || return 1
  if [[ "$count" -gt 1 ]]; then
    echo "Multiple releases claim tag $tag" >&2
    return 1
  fi
  if [[ "$count" -eq 0 ]]; then
    return 4
  fi
  jq -e '.[0] | select(type == "object" and (.assets | type == "array"))' \
    "$matches" >"$release_json" || {
    echo "Release record did not contain the required object and asset array." >&2
    return 1
  }
}

discover_created_draft() {
  local attempt
  local status

  # GitHub can briefly list a new draft under an internal `untagged-*` name after `release create`
  # succeeds. During that window an exact-tag search returns absence even though the draft exists.
  # Retry only discovery: never issue a second create, and fail immediately on API or JSON errors.
  for attempt in 1 2 3 4 5; do
    if discover_release; then
      return 0
    else
      status=$?
    fi
    [[ $status -eq 4 ]] || return "$status"
    [[ $attempt -eq 5 ]] || sleep 2
  done
  echo "Created draft did not become discoverable under tag $tag after 5 attempts." >&2
  return 1
}

validate_metadata() {
  local expected_draft="$1"
  local expected_immutable="$2"
  jq -e \
    --arg tag "$tag" \
    --arg source_commit "$source_commit" \
    --arg title "$title" \
    --arg notes "$notes" \
    --argjson draft "$expected_draft" \
    --argjson immutable "$expected_immutable" \
    '.tag_name == $tag and
     .target_commitish == $source_commit and
     .name == $title and
     .body == $notes and
     .prerelease == false and
     .draft == $draft and
     .immutable == $immutable' \
    "$release_json" >/dev/null || {
    echo "Release metadata does not match the requested immutable source." >&2
    return 1
  }
  if resolved_tag="$(resolve_tag)"; then
    [[ "$resolved_tag" == "$source_commit" ]] || {
      echo "Release tag does not resolve to the requested source commit." >&2
      return 1
    }
  else
    tag_status=$?
    if [[ "$expected_draft" != true || $tag_status -ne 4 ]]; then
      echo "Immutable release tag is missing or could not be verified." >&2
      return 1
    fi
  fi
}

validate_assets() {
  local allow_subset="$1"
  local actual_names
  jq -e '
    (.assets | type == "array") and
    all(.assets[]; (.id | type == "number") and (.name | type == "string")) and
    ([.assets[].id] | unique | length) == (.assets | length) and
    ([.assets[].name] | unique | length) == (.assets | length)
  ' "$release_json" >/dev/null || {
    echo "Release assets contain missing or duplicate stable identities." >&2
    return 1
  }
  actual_names="$(jq -cer '[.assets[].name] | sort' "$release_json")" || return 1
  if [[ "$allow_subset" == false ]]; then
    [[ "$actual_names" == "$expected_names" ]] || {
      echo "Release assets differ: expected $expected_names, got $actual_names" >&2
      return 1
    }
  else
    jq -e --argjson expected "$expected_names" \
      'all(.assets[].name; . as $name | $expected | index($name) != null)' \
      "$release_json" >/dev/null || {
      echo "Draft release contains an unexpected asset." >&2
      return 1
    }
  fi

  local download="$temporary/download"
  rm -rf "$download"
  mkdir -p "$download"
  while IFS= read -r descriptor; do
    name="$(jq -er '.name' <<<"$descriptor")" || return 1
    asset_id="$(jq -er '.id' <<<"$descriptor")" || return 1
    [[ "$asset_id" =~ ^[0-9]+$ ]] || return 1
    expected_path=''
    for asset in "${assets[@]}"; do
      if [[ "$(basename "$asset")" == "$name" ]]; then
        expected_path="$asset"
        break
      fi
    done
    [[ -n "$expected_path" ]]
    gh api -H 'Accept: application/octet-stream' \
      "repos/$repository/releases/assets/$asset_id" >"$download/$name" || return 1
    cmp "$expected_path" "$download/$name" || {
      echo "Release asset bytes differ for $name" >&2
      return 1
    }
    remote_digest="$(jq -r '.digest // empty' <<<"$descriptor")"
    if [[ -n "$remote_digest" ]]; then
      if command -v sha256sum >/dev/null; then
        expected_digest="sha256:$(sha256sum "$expected_path" | awk '{print $1}')"
      else
        expected_digest="sha256:$(shasum -a 256 "$expected_path" | awk '{print $1}')"
      fi
      [[ "$remote_digest" == "$expected_digest" ]] || return 1
    fi
  done < <(jq -c '.assets[]' "$release_json" | LC_ALL=C sort)
}

source_owned_draft() {
  jq -e \
    --arg tag "$tag" \
    --arg source_commit "$source_commit" \
    '.tag_name == $tag and
     .target_commitish == $source_commit and
     .draft == true and
     .immutable == false and
     (.id | type == "number")' "$release_json" >/dev/null || return 1
  if resolved_tag="$(resolve_tag)"; then
    [[ "$resolved_tag" == "$source_commit" ]] || return 1
  else
    tag_status=$?
    [[ $tag_status -eq 4 ]] || return 1
  fi
}

create_draft() {
  tag_state=absent
  if resolved_tag="$(resolve_tag)"; then
    tag_state=present
    [[ "$resolved_tag" == "$source_commit" ]] || {
      echo "Existing release tag does not resolve to the requested source commit." >&2
      return 1
    }
  else
    tag_status=$?
    [[ $tag_status -eq 4 ]] || return 1
  fi
  create=(gh release create "$tag" --repo "$repository" --target "$source_commit" --title "$title" --notes "$notes" --draft)
  [[ "$tag_state" == absent ]] || create+=(--verify-tag)
  "${create[@]}" || return 1
  discover_created_draft || return 1
  validate_metadata true false || return 1
  validate_assets true || return 1
}

release_state=absent
if discover_release; then
  release_state=present
else
  status=$?
  [[ $status -eq 4 ]] || exit "$status"
fi

if [[ "$release_state" == present ]]; then
  immutable="$(jq -r '.immutable // false' "$release_json")" || exit 1
  draft="$(jq -r '.draft' "$release_json")" || exit 1
  [[ "$immutable" == true || "$immutable" == false ]] || exit 1
  [[ "$draft" == true || "$draft" == false ]] || exit 1
  if [[ "$immutable" == true ]]; then
    [[ "$draft" == false ]]
    validate_metadata false true || exit 1
    validate_assets false || exit 1
    echo "Immutable release $tag already matches the requested source and assets."
    exit 0
  fi
  if [[ "$draft" != true ]]; then
    validate_metadata false false || exit 1
    validate_assets false || exit 1
    release_id="$(jq -er '.id' "$release_json")" || exit 1
    [[ "$release_id" =~ ^[0-9]+$ ]] || exit 1
    for _ in 1 2 3 4 5; do
      discover_release || exit 1
      [[ "$(jq -r '.id' "$release_json")" == "$release_id" ]] || exit 1
      current_immutable="$(jq -r '.immutable // false' "$release_json")" || exit 1
      [[ "$current_immutable" == true || "$current_immutable" == false ]] || exit 1
      if [[ "$current_immutable" == true ]]; then
        validate_metadata false true || exit 1
        validate_assets false || exit 1
        echo "Existing release $tag became immutable and matches the requested source and assets."
        exit 0
      fi
      sleep 2
    done
    echo "Refusing to alter an existing non-draft release that remained mutable: $tag" >&2
    exit 1
  fi
  if ! validate_metadata true false || ! validate_assets true; then
    source_owned_draft || {
      echo "Refusing to replace a draft not owned by the exact requested source." >&2
      exit 1
    }
    release_id="$(jq -er '.id' "$release_json")" || exit 1
    gh api --method DELETE "repos/$repository/releases/$release_id" >/dev/null || exit 1
    if discover_release; then
      echo "Deleted draft still appears in release discovery." >&2
      exit 1
    else
      discovery_status=$?
      [[ $discovery_status -eq 4 ]] || exit 1
    fi
    create_draft || exit 1
  fi
else
  create_draft || exit 1
fi

release_id="$(jq -r '.id' "$release_json")"
[[ "$release_id" =~ ^[0-9]+$ ]] || {
  echo "Release did not expose a stable numeric ID." >&2
  exit 1
}

actual_names="$(jq -r '.assets[].name' "$release_json")" || exit 1
for asset in "${assets[@]}"; do
  name="$(basename "$asset")"
  if ! grep -Fxq "$name" <<<"$actual_names"; then
    gh release upload "$tag" --repo "$repository" "$asset" || exit 1
  fi
done

discover_release || exit 1
[[ "$(jq -r '.id' "$release_json")" == "$release_id" ]] || exit 1
validate_metadata true false || exit 1
validate_assets false || exit 1
gh release edit "$tag" --repo "$repository" --draft=false || exit 1

for _ in 1 2 3 4 5; do
  discover_release || exit 1
  [[ "$(jq -r '.id' "$release_json")" == "$release_id" ]] || exit 1
  current_immutable="$(jq -r '.immutable // false' "$release_json")" || exit 1
  [[ "$current_immutable" == true || "$current_immutable" == false ]] || exit 1
  [[ "$current_immutable" != true ]] || break
  sleep 2
done
current_immutable="$(jq -r '.immutable // false' "$release_json")" || exit 1
[[ "$current_immutable" == true ]] || {
  echo "Published release did not become immutable." >&2
  exit 1
}
validate_metadata false true || exit 1
validate_assets false || exit 1
