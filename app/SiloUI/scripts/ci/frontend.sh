#!/usr/bin/env bash
# Frontend checks: typecheck, lint and the Vitest suite of the app, then the
# website demo and the demo films.
#
#   frontend.sh                   everything
#   frontend.sh checks            typecheck, lint, website and demo
#   frontend.sh tests [ARGS...]   the app's Vitest suite; ARGS go to Vitest (for example --shard=1/3)
#
# The app's own dependencies must already be installed (npm ci in app/SiloUI).
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

part=${1:-all}
[ $# -gt 0 ] && shift
case "$part" in all | checks | tests) ;; *) die "unknown part '$part' (all, checks or tests)" ;; esac
[ -d "$APP/node_modules" ] || die "app/SiloUI/node_modules is missing: run npm --prefix app/SiloUI ci"
[ "$part" != checks ] || [ $# -eq 0 ] || die "checks takes no arguments"

checks() {
  cd "$APP" || exit 2
  step typecheck npm run typecheck
  step lint npm run lint
  step website-deps npm_deps "$ROOT/website"
  cd "$ROOT/website" || exit 2
  step website-typecheck npm run typecheck
  step website-tests npm test -- --maxWorkers=2
  step demo-deps npm_deps "$ROOT/demo"
  cd "$ROOT/demo" || exit 2
  step demo-typecheck npm run typecheck
  step demo-tests npm test
}

tests() {
  cd "$APP" || exit 2
  step vitest npm test -- "$@"
}

case "$part" in
  all) checks; tests "$@" ;;
  checks) checks ;;
  tests) tests "$@" ;;
esac
finish
