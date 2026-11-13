#!/usr/bin/env bash
# Release-tooling and script tests: the Node script suites, the release
# version and command manifest checks, every scripts/test_*.py module, the
# isolation eval self-tests (Linux only: they read /proc/self/mountinfo) and
# the hosted reviewer service tests. The Python tests run through uv with
# pinned packages: PyYAML for the workflow guard, and
# reviewer-service/requirements-dev.txt for the reviewer service.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$APP" || exit 2
[ -d node_modules ] || die "app/SiloUI/node_modules is missing: run npm --prefix app/SiloUI ci"
command -v uv >/dev/null 2>&1 || die "uv is not installed (https://docs.astral.sh/uv/getting-started/installation/)"
step release-tests npm run test:release
step release-version python3 scripts/validate-release-version.py
step command-manifest python3 scripts/check-command-manifest.py
step script-tests uv run --no-project --python python3 --with pyyaml==6.0.3 python scripts/run-script-tests.py
if [ "$(uname -s)" = Linux ]; then
  step evals python3 -m unittest discover -s ../../docs/evals -p 'test_*.py'
else
  echo "== evals: skipped (they run on Linux only)"
fi
step reviewer-service uv run --no-project --python python3 --with-requirements ../../reviewer-service/requirements-dev.txt \
  python -m pytest -q ../../reviewer-service
finish
