#!/usr/bin/env bash
# The macOS Rust job: release signing with disposable fixtures, Rust
# formatting and every workspace test.
#
#   macos.sh          every part
#   macos.sh checks   signing fixtures and formatting; needs no native runtime
#   macos.sh tests    every workspace test; needs the prepared native runtime
#                     and GitHub build configuration (real or the release
#                     guide's synthetic values)
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

part=${1:-all}
[ $# -le 1 ] || die "takes at most one part"
case "$part" in all | checks | tests) ;; *) die "unknown part '$part' (all, checks or tests)" ;; esac

checks() {
  step macos-release python3 -m unittest discover -s scripts -p 'test_macos_release.py'
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
