#!/bin/bash
# Applies each patch set to upstream one patch at a time and, after each, builds for Linux, macOS
# and Windows and runs the Go tests of the packages the series touches. Then runs upstream's
# golangci-lint (make lint) on the last tree of each set.
# usage: check.sh       Needs Go (it fetches Go 1.26 itself) and git. ICMP tests need
#                       net.ipv4.ping_group_range to include your group, as upstream's CI sets it.
set -e
H=$(cd $(dirname $0); pwd)
UPSTREAM=197eea05; PR750=4f8f102b
export GOTOOLCHAIN=auto REQUIRE_ICMP_SOCKET=1
S=$(mktemp -d); git clone -q https://github.com/containers/gvisor-tap-vsock.git $S/gv; cd $S/gv; git fetch -q origin pull/750/head
step() {
  for os in linux darwin windows; do GOOS=$os go build ./...; done
  go test ./cmd/gvproxy/ ./pkg/... > $S/test.log 2>&1 || { cat $S/test.log; echo "FAILED after: $1"; exit 1; }
  echo "ok: $1"
}
run() {  # <base> <patch dir>
  git -c advice.detachedHead=false checkout -q $1; echo "== $2 on $1"
  for p in $H/patches/$2/*.patch; do git -c user.name=check -c user.email=check@localhost am -q $p; step "$(basename $p)"; done
  make lint
}
run $UPSTREAM on-main
run $PR750 on-750
rm -rf $S
