#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
publisher="$repository_root/scripts/oci/publish-immutable-release.sh"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

export GITHUB_REPOSITORY=MySproutOS/Deployment-Templates
export GITHUB_SHA=ca6c6168d49522d2deb9433e73fba3bc6f65f74a
export GH_TOKEN=test-only

mkdir -p "$temporary/bin"
mock="$temporary/bin/gh"
cat >"$mock" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail

state="${MOCK_GH_STATE:?}"
command="$1"
shift
printf '%s %s\n' "$command" "$*" >>"$state/calls"

release_json() {
  local release="$state/release.json"
  local assets='[]'
  if [[ -f "$state/become-immutable-after" ]]; then
    count=0
    [[ ! -f "$state/discovery-count" ]] || count="$(cat "$state/discovery-count")"
    count=$((count + 1))
    printf '%s\n' "$count" >"$state/discovery-count"
    threshold="$(cat "$state/become-immutable-after")"
    if [[ "$count" -ge "$threshold" ]]; then
      jq '.draft=false | .immutable=true' "$release" >"$state/next.json"
      mv "$state/next.json" "$release"
      rm "$state/become-immutable-after"
    fi
  fi
  if [[ -d "$state/assets" ]]; then
    asset_id=100
    while IFS= read -r path; do
      asset_id=$((asset_id + 1))
      if command -v sha256sum >/dev/null; then
        digest="sha256:$(sha256sum "$path" | awk '{print $1}')"
      else
        digest="sha256:$(shasum -a 256 "$path" | awk '{print $1}')"
      fi
      assets="$(jq -c --argjson id "$asset_id" --arg name "$(basename "$path")" --arg digest "$digest" \
        '. + [{id:$id,name:$name,digest:$digest}]' <<<"$assets")"
    done < <(find "$state/assets" -type f | LC_ALL=C sort)
  fi
  jq --argjson assets "$assets" '.assets=$assets' "$release"
}

case "$command" in
  api)
    paginate=false
    slurp=false
    method=GET
    while [[ $# -gt 0 && "$1" == -* ]]; do
      case "$1" in
        --paginate) paginate=true ;;
        --slurp) slurp=true ;;
        --method) method="$2"; shift ;;
        -H) shift ;;
        *) exit 2 ;;
      esac
      shift
    done
    endpoint="$1"
    if [[ "$endpoint" == */releases\?per_page=100 ]]; then
      [[ "$paginate" == true && "$slurp" == true ]]
      if [[ -f "$state/api-error" ]]; then
        echo 'gh: server error (HTTP 500)' >&2
        exit 1
      fi
      if [[ -f "$state/malformed-response" ]]; then
        printf '{not-json\n'
        exit 0
      fi
      if [[ ! -f "$state/release.json" ]]; then
        printf '[[]]\n'
      else
        printf '[[%s]]\n' "$(release_json)"
      fi
    elif [[ "$endpoint" == */git/ref/tags/* ]]; then
      if [[ ! -f "$state/tag.json" ]]; then
        echo 'gh: Not Found (HTTP 404)' >&2
        exit 1
      fi
      cat "$state/tag.json"
    elif [[ "$endpoint" == */git/tags/* ]]; then
      object_sha="${endpoint##*/}"
      cat "$state/annotated-$object_sha.json"
    elif [[ "$endpoint" == */releases/assets/* ]]; then
      requested_id="${endpoint##*/}"
      asset_id=100
      while IFS= read -r path; do
        asset_id=$((asset_id + 1))
        if [[ "$requested_id" == "$asset_id" ]]; then
          cat "$path"
          exit 0
        fi
      done < <(find "$state/assets" -type f | LC_ALL=C sort)
      exit 1
    elif [[ "$endpoint" == */releases/* && "$method" == DELETE ]]; then
      rm -f "$state/release.json"
      rm -rf "$state/assets"
    else
      exit 2
    fi
    ;;
  release)
    operation="$1"
    shift
    case "$operation" in
      create)
        tag="$1"
        shift
        target=''
        title=''
        notes=''
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --target) target="$2"; shift 2 ;;
            --title) title="$2"; shift 2 ;;
            --notes) notes="$2"; shift 2 ;;
            --repo) shift 2 ;;
            --draft|--verify-tag) shift ;;
            *) exit 2 ;;
          esac
        done
        [[ ! -f "$state/release.json" ]]
        mkdir -p "$state/assets"
        jq -n --arg tag "$tag" --arg target "$target" --arg title "$title" --arg notes "$notes" \
          '{id:1,tag_name:$tag,target_commitish:$target,name:$title,body:$notes,prerelease:false,draft:true,immutable:false,assets:[]}' \
          >"$state/release.json"
        jq -n --arg sha "$target" '{object:{type:"commit",sha:$sha}}' >"$state/tag.json"
        ;;
      upload)
        shift # tag
        [[ "$1" == --repo ]]
        shift 2
        mkdir -p "$state/assets"
        for asset in "$@"; do cp "$asset" "$state/assets/$(basename "$asset")"; done
        if [[ -f "$state/upload-failure-once" ]]; then
          rm "$state/upload-failure-once"
          exit 1
        fi
        ;;
      edit)
        shift # tag
        jq '.draft=false | .immutable=true' "$state/release.json" >"$state/next.json"
        mv "$state/next.json" "$state/release.json"
        if [[ -f "$state/publish-response-failure-once" ]]; then
          rm "$state/publish-response-failure-once"
          exit 1
        fi
        ;;
      delete)
        rm -f "$state/release.json" "$state/tag.json"
        rm -rf "$state/assets"
        ;;
      *) exit 2 ;;
    esac
    ;;
  *) exit 2 ;;
esac
MOCK
chmod +x "$mock"
export PATH="$temporary/bin:$PATH"

asset_root="$temporary/input"
mkdir -p "$asset_root"
printf 'alpha\n' >"$asset_root/alpha.json"
printf 'beta\n' >"$asset_root/beta.json"
tag=test-release
title='Test release'
notes='Exact notes'

new_state() {
  local name="$1"
  MOCK_GH_STATE="$temporary/$name"
  export MOCK_GH_STATE
  mkdir -p "$MOCK_GH_STATE"
}

seed_release() {
  local draft="$1"
  local immutable="$2"
  local target="${3:-$GITHUB_SHA}"
  mkdir -p "$MOCK_GH_STATE/assets"
  jq -n \
    --arg tag "$tag" --arg target "$target" --arg title "$title" --arg notes "$notes" \
    --argjson draft "$draft" --argjson immutable "$immutable" \
    '{id:1,tag_name:$tag,target_commitish:$target,name:$title,body:$notes,prerelease:false,draft:$draft,immutable:$immutable,assets:[]}' \
    >"$MOCK_GH_STATE/release.json"
  jq -n --arg sha "$target" '{object:{type:"commit",sha:$sha}}' >"$MOCK_GH_STATE/tag.json"
}

run_publisher() {
  "$publisher" "$tag" "$title" "$notes" "$asset_root/alpha.json" "$asset_root/beta.json"
}

expect_failure() {
  set +e
  run_publisher
  result=$?
  set -e
  [[ $result -ne 0 ]]
}

new_state absent
run_publisher
[[ "$(jq -r '.immutable' "$MOCK_GH_STATE/release.json")" == true ]]
cmp "$asset_root/alpha.json" "$MOCK_GH_STATE/assets/alpha.json"
cmp "$asset_root/beta.json" "$MOCK_GH_STATE/assets/beta.json"

new_state immutable
seed_release false true
cp "$asset_root"/*.json "$MOCK_GH_STATE/assets/"
printf '' >"$MOCK_GH_STATE/calls"
run_publisher
if grep -Eq '^release (create|upload|edit)|^api --method DELETE' "$MOCK_GH_STATE/calls"; then
  echo 'Exact immutable reuse performed a mutation.' >&2
  exit 1
fi

new_state exact-tag-only
jq -n --arg sha "$GITHUB_SHA" '{object:{type:"commit",sha:$sha}}' >"$MOCK_GH_STATE/tag.json"
run_publisher

new_state wrong-tag-only
jq -n '{object:{type:"commit",sha:"0000000000000000000000000000000000000000"}}' >"$MOCK_GH_STATE/tag.json"
if ! expect_failure; then
  echo 'Wrong-target tag-only state was accepted.' >&2
  exit 1
fi
[[ ! -f "$MOCK_GH_STATE/release.json" ]]

new_state annotated-tag-only
annotated_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
jq -n --arg sha "$annotated_sha" '{object:{type:"tag",sha:$sha}}' >"$MOCK_GH_STATE/tag.json"
jq -n --arg sha "$GITHUB_SHA" '{object:{type:"commit",sha:$sha}}' >"$MOCK_GH_STATE/annotated-$annotated_sha.json"
run_publisher

new_state partial-draft
seed_release true false
cp "$asset_root/alpha.json" "$MOCK_GH_STATE/assets/"
run_publisher
[[ "$(jq -r '.immutable' "$MOCK_GH_STATE/release.json")" == true ]]
cmp "$asset_root/beta.json" "$MOCK_GH_STATE/assets/beta.json"

new_state wrong-title-draft
seed_release true false
jq '.name="stale title"' "$MOCK_GH_STATE/release.json" >"$MOCK_GH_STATE/next.json"
mv "$MOCK_GH_STATE/next.json" "$MOCK_GH_STATE/release.json"
cp "$asset_root"/*.json "$MOCK_GH_STATE/assets/"
run_publisher
[[ "$(jq -r '.name' "$MOCK_GH_STATE/release.json")" == "$title" ]]

new_state wrong-bytes-draft
seed_release true false
printf 'wrong\n' >"$MOCK_GH_STATE/assets/alpha.json"
run_publisher
cmp "$asset_root/alpha.json" "$MOCK_GH_STATE/assets/alpha.json"

new_state extra-asset-draft
seed_release true false
cp "$asset_root"/*.json "$MOCK_GH_STATE/assets/"
printf 'extra\n' >"$MOCK_GH_STATE/assets/extra.json"
run_publisher
[[ ! -f "$MOCK_GH_STATE/assets/extra.json" ]]

new_state bad-immutable-bytes
seed_release false true
printf 'wrong\n' >"$MOCK_GH_STATE/assets/alpha.json"
cp "$asset_root/beta.json" "$MOCK_GH_STATE/assets/"
if ! expect_failure; then
  echo 'Mismatched immutable asset bytes were accepted.' >&2
  exit 1
fi

new_state bad-immutable-title
seed_release false true
jq '.name="wrong"' "$MOCK_GH_STATE/release.json" >"$MOCK_GH_STATE/next.json"
mv "$MOCK_GH_STATE/next.json" "$MOCK_GH_STATE/release.json"
cp "$asset_root"/*.json "$MOCK_GH_STATE/assets/"
if ! expect_failure; then
  echo 'Mismatched immutable metadata was accepted.' >&2
  exit 1
fi

new_state bad-draft-target
seed_release true false 0000000000000000000000000000000000000000
if ! expect_failure; then
  echo 'Mismatched draft target was accepted.' >&2
  exit 1
fi

new_state mutable-public
seed_release false false
cp "$asset_root"/*.json "$MOCK_GH_STATE/assets/"
if ! expect_failure; then
  echo 'Existing mutable public release was accepted.' >&2
  exit 1
fi

new_state mutable-becomes-immutable
seed_release false false
cp "$asset_root"/*.json "$MOCK_GH_STATE/assets/"
printf '2\n' >"$MOCK_GH_STATE/become-immutable-after"
run_publisher

new_state interrupted-upload
touch "$MOCK_GH_STATE/upload-failure-once"
if ! expect_failure; then
  echo 'Simulated upload response failure unexpectedly succeeded.' >&2
  exit 1
fi
run_publisher
[[ "$(jq -r '.immutable' "$MOCK_GH_STATE/release.json")" == true ]]

new_state interrupted-publish
touch "$MOCK_GH_STATE/publish-response-failure-once"
if ! expect_failure; then
  echo 'Simulated publish response failure unexpectedly succeeded.' >&2
  exit 1
fi
run_publisher
[[ "$(jq -r '.immutable' "$MOCK_GH_STATE/release.json")" == true ]]

new_state api-failure
touch "$MOCK_GH_STATE/api-error"
if ! expect_failure; then
  echo 'Release API failure was treated as absence.' >&2
  exit 1
fi

new_state malformed-api
touch "$MOCK_GH_STATE/malformed-response"
if ! expect_failure; then
  echo 'Malformed release API JSON was treated as absence.' >&2
  exit 1
fi
[[ ! -f "$MOCK_GH_STATE/release.json" ]]

echo 'immutable release recovery tests passed'
