#!/usr/bin/env bash
# silo-runner, the Go helper: formatting, vet and the race-enabled tests.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

gofmt_clean() {
  local unformatted
  unformatted=$(gofmt -l .) || return 1
  [ -z "$unformatted" ] || { printf 'not gofmt-formatted:\n%s\n' "$unformatted" >&2; return 1; }
}

cd "$APP/runner" || exit 2
step gofmt gofmt_clean
step vet go vet ./...
step test go test -race -count=1 ./...
finish
