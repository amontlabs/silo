#!/usr/bin/env bash
# The macOS Rust job: release signing with disposable fixtures, Rust
# formatting, a check that the shipped binaries do not build with
# test-support, and every workspace test.
#
#   macos.sh          every part
#   macos.sh checks   signing fixtures, formatting and the test-support check;
#                     needs no native runtime
#   macos.sh tests    every workspace test; needs the prepared native runtime
#                     and GitHub build configuration (real or the release
#                     guide's synthetic values)
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

part=${1:-all}
[ $# -le 1 ] || die "takes at most one part"
case "$part" in all | checks | tests) ;; *) die "unknown part '$part' (all, checks or tests)" ;; esac

checks() {
  step macos-release python3 -m unittest discover -s scripts -p 'test_macos_release.py'
  step shipped-without-test-support no_test_support
  step rustfmt cargo fmt --check -p silo-api -p silo-cli -p silo-client -p silo-host -p silo-container-shim -p silo-ui -p silo-engine -p silod
}

tests() {
  # The workspace has no doctests, which nextest does not run.
  step rust cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" --manifest-path Cargo.toml --workspace
}

cd "$APP" || exit 2
case "$part" in
  all) checks; tests ;;
  *) "$part" ;;
esac
finish
