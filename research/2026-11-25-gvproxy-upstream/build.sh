#!/bin/bash
# Builds what check.sh and the suite run, into <bin-dir>:
#   gvproxy-stock    upstream containers/gvisor-tap-vsock at the commit below
#   gvproxy-on-main  the same with patches/on-main (five patches that need nothing else)
#   gvproxy-series   the same with upstream PR #750 (the dial filter) and patches/on-750
#   gvforwarder, standin-boundary (#380's stand-in for Silo's boundary)
# usage: build.sh <bin-dir>      Needs Go (it fetches Go 1.26 itself) and git.
set -e
H=$(cd $(dirname $0); pwd); B=$(mkdir -p $1; cd $1; pwd)
UPSTREAM=197eea05   # containers/gvisor-tap-vsock main on 25 November 2026, v0.9.0 and 52 commits
PR750=4f8f102b      # head of upstream PR #750, "Add a dial filter and a DNS answer observer for embedders"
export GOTOOLCHAIN=auto
S=$(mktemp -d); git clone -q https://github.com/containers/gvisor-tap-vsock.git $S/gv; cd $S/gv
git fetch -q origin pull/750/head; git -c advice.detachedHead=false checkout -q $UPSTREAM
go build -o $B/gvproxy-stock ./cmd/gvproxy; go build -o $B/gvforwarder ./cmd/vm
git -c user.name=build -c user.email=build@localhost am -q $H/patches/on-main/*.patch
go build -o $B/gvproxy-on-main ./cmd/gvproxy
git checkout -q $PR750
git -c user.name=build -c user.email=build@localhost am -q $H/patches/on-750/*.patch
go build -o $B/gvproxy-series ./cmd/gvproxy
(cd $H/boundary && go build -o $B/standin-boundary .)
rm -rf $S; ls $B
