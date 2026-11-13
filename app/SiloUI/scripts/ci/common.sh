# Sourced by every script in this folder. CI calls these scripts, and they run
# the same way on a developer machine.

set -uo pipefail

CI_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
APP=$(cd "$CI_DIR/../.." && pwd)
ROOT=$(cd "$APP/../.." && pwd)

# Every `cargo nextest run` takes these flags: every test runs to the end, a
# test runs once, a test that fails then passes fails the run, and no
# configured default filter hides a test.
SILO_NEXTEST_FLAGS=(--locked --profile ci --no-fail-fast --retries 0 --flaky-result fail --ignore-default-filter)

failed=()

# step NAME COMMAND...: runs one check; a failure is recorded and the later
# checks still run.
step() {
  local name=$1
  shift
  local started=$SECONDS
  echo "== $name: $*"
  if "$@"; then
    echo "== $name: ok ($((SECONDS - started)) s)"
  else
    echo "== $name: FAILED ($((SECONDS - started)) s)" >&2
    failed+=("$name")
  fi
}

# finish: prints the failed checks and exits non-zero when there is one.
finish() {
  if [ "${#failed[@]}" -gt 0 ]; then
    echo "failed: ${failed[*]}" >&2
    exit 1
  fi
  echo "all checks passed"
}

die() {
  echo "error: $*" >&2
  exit 2
}

# npm_deps DIR: installs DIR's npm dependencies when they are missing or older
# than its package-lock.json.
npm_deps() {
  local dir=$1
  if [ ! -f "$dir/node_modules/.package-lock.json" ] || [ "$dir/package-lock.json" -nt "$dir/node_modules/.package-lock.json" ]; then
    (cd "$dir" && npm ci)
  fi
}
