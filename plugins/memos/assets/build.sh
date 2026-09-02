#!/bin/sh
set -eu

expected_commit="22a5f3385b9fc5bdf282eb597aa3db79798aa5ab"
version="0.30.0"

test "$(go env GOVERSION)" = "go1.27.0"
# The single quotes intentionally pass this JavaScript expression to Node verbatim.
# shellcheck disable=SC2016
test "$(node -p 'process.versions.node.split(`.`)[0]')" = "24"
test "$(corepack pnpm@11.0.1 --version)" = "11.0.1"
corepack pnpm@11.0.1 --dir web install --frozen-lockfile
corepack pnpm@11.0.1 --dir web release

mkdir -p .sproutos/dist .sproutos/migration
CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build \
  -trimpath \
  -tags netgo,osusergo \
  -ldflags "-s -w -X github.com/usememos/memos/internal/version.Version=${version} -X github.com/usememos/memos/internal/version.Commit=${expected_commit} -extldflags '-static'" \
  -o .sproutos/dist/memos \
  ./cmd/memos
CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build \
  -trimpath \
  -tags netgo,osusergo \
  -ldflags "-s -w -X github.com/usememos/memos/internal/version.Version=${version} -X github.com/usememos/memos/internal/version.Commit=${expected_commit} -extldflags '-static'" \
  -o .sproutos/migration/bootstrap \
  ./cmd/sproutos-migrate
# AWS custom runtimes always start an executable named `bootstrap` from the deployment archive
# root. The Lambda Handler field is not an alternate entrypoint for `provided.al2023`.
cp sproutos/run.sh .sproutos/dist/bootstrap
chmod 0755 .sproutos/dist/memos .sproutos/dist/bootstrap .sproutos/migration/bootstrap
