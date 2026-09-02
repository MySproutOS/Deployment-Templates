#!/usr/bin/env bash
set -euo pipefail

workspace="${1:?usage: runtime-e2e.sh <transformed-umami-workspace>}"
test "$(uname -s)" = "Linux"
test "$(uname -m)" = "aarch64"
command -v docker >/dev/null
command -v jq >/dev/null

scratch="$(mktemp -d "${TMPDIR:-/tmp}/sprout-umami-runtime.XXXXXX")"
suffix="$(printf '%s-%s' "${GITHUB_RUN_ID:-local}" "$$" | tr -cd '[:alnum:]_.-')"
database_container="sprout-umami-db-$suffix"
database_volume="sprout-umami-db-$suffix"
app_pid=""

cleanup() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" >/dev/null 2>&1 || true
    wait "$app_pid" >/dev/null 2>&1 || true
  fi
  docker rm --force "$database_container" >/dev/null 2>&1 || true
  docker volume rm "$database_volume" >/dev/null 2>&1 || true
  rm -rf -- "$scratch"
}
trap cleanup EXIT

generated_password="SproutOS-runtime-e2e-generated-password"
owner_password="SproutOS-runtime-e2e-owner-password"
app_secret="SproutOS-runtime-e2e-app-secret-32"
website_id="11111111-1111-4111-8111-111111111111"
database_host="${SPROUTOS_RUNTIME_E2E_DATABASE_HOST:-127.0.0.1}"

docker volume create "$database_volume" >/dev/null

start_database() {
  local published_port="${1:-}"
  local publish=(--publish 127.0.0.1::5432)
  local database_logs
  if [[ -n "$published_port" ]]; then
    publish=(--publish "127.0.0.1:$published_port:5432")
  fi
  docker run --detach --name "$database_container" \
    "${publish[@]}" \
    --env POSTGRES_USER=umami \
    --env POSTGRES_PASSWORD=runtime-e2e \
    --env POSTGRES_DB=umami \
    --volume "$database_volume:/var/lib/postgresql" \
    postgres:18-bookworm >/dev/null
  for _ in {1..60}; do
    database_logs="$(docker logs "$database_container" 2>&1)"
    if docker exec "$database_container" pg_isready --username umami --dbname umami >/dev/null 2>&1 &&
      { grep -Fq 'PostgreSQL init process complete; ready for start up.' <<<"$database_logs" ||
        grep -Fq 'Skipping initialization' <<<"$database_logs"; }; then
      return
    fi
    sleep 1
  done
  docker logs "$database_container" >&2
  return 1
}

run_migration() {
  (
    cd "$workspace/.sproutos/build/migration"
    env \
      DATABASE_URL="$database_url" \
      UMAMI_ADMIN_PASSWORD="$generated_password" \
      node --input-type=module --eval 'import("./index.mjs").then(module => module.handler()).then(result => { if (!result?.ok) process.exit(1); })'
  )
}

set_administrator_hash() {
  local hash="$1"
  printf '%s\n' "UPDATE \"user\" SET \"password\" = '$hash' WHERE \"user_id\" = '41e2b680-648e-4b09-bcd7-3e2b10c06264';" |
    docker exec --interactive "$database_container" psql --username umami --dbname umami --quiet >/dev/null
}

administrator_hash() {
  docker exec "$database_container" psql --username umami --dbname umami --tuples-only --no-align \
    --command "SELECT \"password\" FROM \"user\" WHERE \"user_id\" = '41e2b680-648e-4b09-bcd7-3e2b10c06264';"
}

expect_migration_failure() {
  local expected="$1"
  local log="$2"
  set +e
  run_migration >"$log" 2>&1
  local status=$?
  set -e
  test "$status" -ne 0
  grep -Fq "$expected" "$log"
}

stage_release() {
  local destination="$1"
  mkdir -p "$destination/.next"
  cp -R "$workspace/.next/standalone/." "$destination/"
  cp -R "$workspace/.next/static" "$destination/.next/static"
  if [[ -d "$workspace/public" ]]; then
    cp -R "$workspace/public" "$destination/public"
  fi
}

start_app() {
  local release="$1"
  local log="$2"
  (
    cd "$release"
    exec env \
      APP_SECRET="$app_secret" \
      DATABASE_URL="$database_url" \
      HOSTNAME=127.0.0.1 \
      PORT="$app_port" \
      UMAMI_ADMIN_PASSWORD="$generated_password" \
      node server.js
  ) >"$log" 2>&1 &
  app_pid=$!
  for _ in {1..60}; do
    if curl --fail --silent --show-error "http://127.0.0.1:$app_port/api/heartbeat" >"$scratch/heartbeat.json"; then
      return
    fi
    if ! kill -0 "$app_pid" >/dev/null 2>&1; then
      cat "$log" >&2
      return 1
    fi
    sleep 1
  done
  cat "$log" >&2
  return 1
}

stop_app() {
  kill "$app_pid"
  wait "$app_pid" || true
  app_pid=""
}

login_status() {
  local password="$1"
  local output="$2"
  curl --silent --show-error --output "$output" --write-out '%{http_code}' \
    --header 'content-type: application/json' \
    --data "$(jq --null-input --compact-output --arg password "$password" '{username:"admin", password:$password}')" \
    "http://127.0.0.1:$app_port/api/auth/login"
}

start_database
database_port="$(docker port "$database_container" 5432/tcp | sed -E 's/^.*:([0-9]+)$/\1/')"
[[ "$database_port" =~ ^[0-9]+$ ]]
database_url="postgresql://umami:runtime-e2e@$database_host:$database_port/umami"

# Missing, short, and public-default generated credentials fail before even creating Prisma's
# migration table.
docker exec "$database_container" createdb --username umami umami_invalid
set +e
(
  cd "$workspace/.sproutos/build/migration"
  DATABASE_URL="postgresql://umami:runtime-e2e@$database_host:$database_port/umami_invalid" \
    node --input-type=module --eval 'import("./index.mjs").then(module => module.handler())'
) >"$scratch/missing-password.log" 2>&1
missing_password_status=$?
set -e
test "$missing_password_status" -ne 0
grep -Fq 'UMAMI_ADMIN_PASSWORD must contain at least 32 bytes' "$scratch/missing-password.log"
test "$(docker exec "$database_container" psql --username umami --dbname umami_invalid --tuples-only --no-align --command "select to_regclass('_prisma_migrations') is null;")" = "t"
for invalid_password in short umami; do
  set +e
  (
    cd "$workspace/.sproutos/build/migration"
    DATABASE_URL="postgresql://umami:runtime-e2e@$database_host:$database_port/umami_invalid" \
      UMAMI_ADMIN_PASSWORD="$invalid_password" \
      node --input-type=module --eval 'import("./index.mjs").then(module => module.handler())'
  ) >"$scratch/invalid-password-$invalid_password.log" 2>&1
  invalid_password_status=$?
  set -e
  test "$invalid_password_status" -ne 0
  grep -Fq 'UMAMI_ADMIN_PASSWORD must contain at least 32 bytes' "$scratch/invalid-password-$invalid_password.log"
  test "$(docker exec "$database_container" psql --username umami --dbname umami_invalid --tuples-only --no-align --command "select to_regclass('_prisma_migrations') is null;")" = "t"
done

run_migration
test "$(docker exec "$database_container" psql --username umami --dbname umami --tuples-only --no-align --command 'select count(*) from _prisma_migrations where finished_at is not null and rolled_back_at is null;')" = "24"

app_port="$(node --eval 'const net = require("node:net"); const server = net.createServer(); server.listen(0, "127.0.0.1", () => { console.log(server.address().port); server.close(); });')"
[[ "$app_port" =~ ^[0-9]+$ ]]
stage_release "$scratch/release-one"

# The application wrapper rejects a missing or weakened APP_SECRET before importing Umami.
set +e
(
  cd "$scratch/release-one"
  env -u APP_SECRET DATABASE_URL="$database_url" node server.js
) >"$scratch/missing-app-secret.log" 2>&1
missing_app_secret_status=$?
(
  cd "$scratch/release-one"
  APP_SECRET=short DATABASE_URL="$database_url" node server.js
) >"$scratch/short-app-secret.log" 2>&1
short_app_secret_status=$?
set -e
test "$missing_app_secret_status" -ne 0
test "$short_app_secret_status" -ne 0
grep -Fq 'APP_SECRET must contain at least 32 bytes' "$scratch/missing-app-secret.log"
grep -Fq 'APP_SECRET must contain at least 32 bytes' "$scratch/short-app-secret.log"

start_app "$scratch/release-one" "$scratch/release-one.log"

test "$(login_status umami "$scratch/default-login.json")" = "401"
test "$(login_status "$generated_password" "$scratch/generated-login.json")" = "200"
generated_token="$(jq --raw-output --exit-status '.token' "$scratch/generated-login.json")"

curl --fail --silent --show-error \
  --header "authorization: Bearer $generated_token" \
  --header 'content-type: application/json' \
  --data "$(jq --null-input --compact-output --arg id "$website_id" '{id:$id, name:"SproutOS persistence probe", domain:"persist.example"}')" \
  "http://127.0.0.1:$app_port/api/websites" >"$scratch/website.json"
test "$(jq --raw-output '.id' "$scratch/website.json")" = "$website_id"

curl --fail --silent --show-error \
  --header "authorization: Bearer $generated_token" \
  --header 'content-type: application/json' \
  --data "$(jq --null-input --compact-output --arg current "$generated_password" --arg replacement "$owner_password" '{currentPassword:$current, newPassword:$replacement}')" \
  "http://127.0.0.1:$app_port/api/me/password" >"$scratch/password-change.json"
test "$(login_status "$generated_password" "$scratch/generated-login-after-change.json")" = "401"
test "$(login_status "$owner_password" "$scratch/owner-login.json")" = "200"

# Malformed hashes and a differently salted hash of the public default fail closed instead of
# being mistaken for an owner-changed password. Restore the valid owner hash after each probe.
owner_hash="$(administrator_hash)"
set_administrator_hash 'not-a-bcrypt-hash'
expect_migration_failure 'unsupported password hash state' "$scratch/malformed-hash.log"
set_administrator_hash "$owner_hash"
unsupported_cost_hash="$(
  cd "$workspace/.sproutos/build/migration"
  node --input-type=module --eval 'import bcrypt from "bcryptjs"; console.log(await bcrypt.hash("owner-password", 11));'
)"
set_administrator_hash "$unsupported_cost_hash"
expect_migration_failure 'unsupported password hash state' "$scratch/unsupported-cost.log"
set_administrator_hash "$owner_hash"
salted_default_hash="$(
  cd "$workspace/.sproutos/build/migration"
  node --input-type=module --eval 'import bcrypt from "bcryptjs"; console.log(await bcrypt.hash("umami", 12));'
)"
set_administrator_hash "$salted_default_hash"
expect_migration_failure 'still uses the public upstream default password' "$scratch/resalted-default.log"
set_administrator_hash "$owner_hash"

# A second controlled migration is idempotent and never resets a valid owner-changed password.
run_migration
test "$(login_status "$generated_password" "$scratch/generated-login-after-migrate.json")" = "401"
test "$(login_status "$owner_password" "$scratch/owner-login-after-migrate.json")" = "200"
owner_token="$(jq --raw-output --exit-status '.token' "$scratch/owner-login-after-migrate.json")"
stop_app

# Restart PostgreSQL on the same durable volume, then replace the whole application process and
# release tree. The old token and record must survive both boundaries.
docker rm --force "$database_container" >/dev/null
start_database "$database_port"
stage_release "$scratch/release-two"
start_app "$scratch/release-two" "$scratch/release-two.log"
test "$(login_status umami "$scratch/default-login-after-restart.json")" = "401"
test "$(login_status "$owner_password" "$scratch/owner-login-after-restart.json")" = "200"
curl --fail --silent --show-error \
  --header "authorization: Bearer $owner_token" \
  "http://127.0.0.1:$app_port/api/websites?pageSize=10" >"$scratch/websites-after-restart.json"
test "$(jq --arg id "$website_id" '[.data[] | select(.id == $id)] | length' "$scratch/websites-after-restart.json")" = "1"

curl --fail --silent --show-error "http://127.0.0.1:$app_port/login" >"$scratch/login.html"
static_path="$(grep -Eo '/_next/static/[^\" ]+' "$scratch/login.html" | head -n 1)"
test -n "$static_path"
curl --fail --silent --show-error "http://127.0.0.1:$app_port$static_path" >/dev/null

# Removing the seeded account after establishing another administrator is an intentional owner
# action. The controlled migration must remain idempotent and must not recreate that account.
docker exec "$database_container" psql --username umami --dbname umami --quiet \
  --command "DELETE FROM \"user\" WHERE \"user_id\" = '41e2b680-648e-4b09-bcd7-3e2b10c06264';" >/dev/null
run_migration
test -z "$(administrator_hash)"

printf 'Umami runtime E2E passed: secret preflights, hash fail-closed checks, 24 migrations, replacement release, durable PostgreSQL data\n'
