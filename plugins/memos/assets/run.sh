#!/bin/sh
set -eu

: "${MEMOS_DSN:?MEMOS_DSN is required}"
: "${PORT:?PORT is required}"

export MEMOS_DRIVER=postgres
export MEMOS_DATA=/tmp/memos
export MEMOS_PORT="$PORT"
# The separately published migration function is the sole schema writer. The application must
# never race a rollout peer by applying migrations during cold start.
export MEMOS_SPROUTOS_CONTROLLED_MIGRATIONS=true

exec "$(dirname "$0")/memos"
