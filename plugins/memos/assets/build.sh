#!/bin/sh
set -eu

expected_commit="22a5f3385b9fc5bdf282eb597aa3db79798aa5ab"
version="0.30.0"

test "$(go env GOVERSION)" = "go1.27.0"
test "$(node -p 'process.versions.node.split(`.`)[0]')" = "24"
test "$(corepack pnpm --version)" = "11.0.1"
corepack pnpm --dir web install --frozen-lockfile
corepack pnpm --dir web release

mkdir -p .sproutos/dist
CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build \
  -trimpath \
  -tags netgo,osusergo \
  -ldflags "-s -w -X github.com/usememos/memos/internal/version.Version=${version} -X github.com/usememos/memos/internal/version.Commit=${expected_commit} -extldflags '-static'" \
  -o .sproutos/dist/memos \
  ./cmd/memos
cp sproutos/run.sh .sproutos/dist/run.sh
chmod 0755 .sproutos/dist/memos .sproutos/dist/run.sh
