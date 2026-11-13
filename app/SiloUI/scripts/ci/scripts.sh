#!/usr/bin/env bash
# Release-tooling and script tests: the Node script suites, the release
# version and command manifest checks, every scripts/test_*.py module, the
# isolation eval self-tests (Linux only: they read /proc/self/mountinfo) and
# the hosted reviewer service tests (pytest from
# reviewer-service/requirements-dev.txt must be installed).
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$APP" || exit 2
[ -d node_modules ] || die "app/SiloUI/node_modules is missing: run npm --prefix app/SiloUI ci"
step release-tests npm run test:release
step release-version python3 scripts/validate-release-version.py
step command-manifest python3 scripts/check-command-manifest.py
step script-tests python3 scripts/run-script-tests.py
if [ "$(uname -s)" = Linux ]; then
  step evals python3 -m unittest discover -s ../../docs/evals -p 'test_*.py'
else
  echo "== evals: skipped (they run on Linux only)"
fi
if python3 -c 'import pytest' 2>/dev/null; then
  step reviewer-service python3 -m pytest -q ../../reviewer-service
else
  echo "== reviewer-service: FAILED (pytest is missing: python3 -m pip install -r reviewer-service/requirements-dev.txt)" >&2
  failed+=(reviewer-service)
fi
finish
