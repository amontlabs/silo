#!/usr/bin/env bash
# Relative-link check of the first-party documentation with lychee in offline
# mode, so web links never make it flaky. Uses $LYCHEE when set, otherwise
# lychee on PATH; it must be version 0.24.2.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

LYCHEE_VERSION=0.24.2
lychee=${LYCHEE:-lychee}
command -v "$lychee" >/dev/null 2>&1 \
  || die "lychee $LYCHEE_VERSION is not installed (https://github.com/lycheeverse/lychee/releases/tag/lychee-v$LYCHEE_VERSION); put it on PATH or set LYCHEE"
version=$("$lychee" --version 2>&1)
[ "$version" = "lychee $LYCHEE_VERSION" ] || die "lychee $LYCHEE_VERSION is required, found: $version"

# The globs cover every tracked first-party Markdown file (test_ci_coverage.py checks it).
patterns=(
  '*.md'
  'docs/**/*.md'
  '.atelier/**/*.md'
  'app/SiloUI/runner/**/*.md'
  'app/SiloUI/*.md'
  'app/SiloUI/api/**/*.md'
  'app/SiloUI/tests/**/*.md'
  'app/SiloUI/docs/**/*.html'
  'app/SiloUI/src/**/*.md'
  'app/SiloUI/src-tauri/guest/**/*.md'
  'app/SiloUI/src-tauri/macos-gateway/*.md'
  'app/SiloUI/.changeset/*.md'
  'artifacts/**/*.md'
  'website/*.md'
  'demo/*.md'
  'reviewer-service/*.md'
)

cd "$ROOT" || exit 2
step links "$lychee" --offline --no-progress "${patterns[@]}"
finish
