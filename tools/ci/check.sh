#!/usr/bin/env bash
# The whole check of this repository. CI and the merge gate both run it.
#
#   check.sh          every part, in order
#   check.sh PART     one part: deps, typecheck, tests, build, plan or links
#
# The links part needs lychee 0.24.2: $LYCHEE when set, otherwise lychee on PATH.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

LYCHEE_VERSION=0.24.2
WEBSITE=$ROOT/website

deps() { step website-deps npm_deps "$WEBSITE"; }

typecheck() { step website-typecheck npm --prefix "$WEBSITE" run typecheck; }

# Every test suite, in order: the website's, the Workshop tool's, the plan tool's.
tests() {
  step website-tests npm --prefix "$WEBSITE" test -- --maxWorkers=2
  step workshop-tests npm --prefix "$ROOT/tools/workshop" test
  step plan-tests npm --prefix "$ROOT/tools/plan" test
}

build() { step website-build npm --prefix "$WEBSITE" run build; }

plan() { step plan-check "$ROOT/tools/plan/bin/plan" check; }

# Relative links in every tracked Markdown file, offline so web links never
# make the check flaky.
lychee_links() {
  local lychee=${LYCHEE:-lychee} version files=()
  if ! command -v "$lychee" >/dev/null 2>&1; then
    echo "lychee $LYCHEE_VERSION is not installed (https://github.com/lycheeverse/lychee/releases/tag/lychee-v$LYCHEE_VERSION); put it on PATH or set LYCHEE" >&2
    return 1
  fi
  version=$("$lychee" --version 2>&1)
  if [ "$version" != "lychee $LYCHEE_VERSION" ]; then
    echo "lychee $LYCHEE_VERSION is required, found: $version" >&2
    return 1
  fi
  while IFS= read -r -d '' file; do files+=("$file"); done < <(git -C "$ROOT" ls-files -z -- '*.md')
  if [ "${#files[@]}" -eq 0 ]; then
    echo "no tracked Markdown file found" >&2
    return 1
  fi
  (cd "$ROOT" && "$lychee" --offline --no-progress "${files[@]}")
}

links() { step links lychee_links; }

part=${1:-all}
[ $# -le 1 ] || die "usage: check.sh [deps|typecheck|tests|build|plan|links]"
case "$part" in
  all) deps; typecheck; tests; build; plan; links ;;
  deps | typecheck | tests | build | plan | links) "$part" ;;
  *) die "unknown part '$part' (deps, typecheck, tests, build, plan or links)" ;;
esac
finish
