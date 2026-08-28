#!/usr/bin/env bash
set -euo pipefail

platforms='[
  {"runner":"ubuntu-24.04",     "target":"x86_64-unknown-linux-musl",   "os":"linux",   "arch":"amd64", "suffix":""},
  {"runner":"ubuntu-24.04-arm", "target":"aarch64-unknown-linux-musl",  "os":"linux",   "arch":"arm64", "suffix":""},
  {"runner":"macos-15-intel",   "target":"x86_64-apple-darwin",        "os":"darwin",  "arch":"amd64", "suffix":""},
  {"runner":"macos-15",         "target":"aarch64-apple-darwin",       "os":"darwin",  "arch":"arm64", "suffix":""},
  {"runner":"windows-2025",     "target":"x86_64-pc-windows-msvc",    "os":"windows", "arch":"amd64", "suffix":".exe"}
]'
kinds='["success","stdout-flood","fork-timeout"]'
jq -cn --argjson platforms "$platforms" --argjson kinds "$kinds" \
  '{include:[$kinds[] as $kind | $platforms[] | . + {kind:$kind,bin:$kind}]}'
