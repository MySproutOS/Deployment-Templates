#!/bin/sh
set -eu

: "${MEMOS_DSN:?MEMOS_DSN is required}"
: "${PORT:?PORT is required}"

export MEMOS_DRIVER=postgres
export MEMOS_DATA=/tmp/memos
export MEMOS_PORT="$PORT"

exec "$(dirname "$0")/memos"
