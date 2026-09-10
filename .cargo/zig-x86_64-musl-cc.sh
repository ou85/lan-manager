#!/usr/bin/env bash
set -eu

args=()
for arg in "$@"; do
  [ "$arg" = "--target=x86_64-unknown-linux-musl" ] || args+=("$arg")
done
exec zig cc -target x86_64-linux-musl "${args[@]}"
