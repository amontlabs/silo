#!/bin/bash
# Builds everything the suite and the bench run, into <bin-dir>:
#   gvproxy-stock   upstream gvisor-tap-vsock at the commit below
#   gvproxy-363     the same with #363's -sealed patch (research/2026-11-24-red-team-fixes/network/)
#   gvproxy-silo    the same with gvproxy-silo.patch, this folder's
#   gvforwarder, fwd, fwdgram, standin-boundary
# usage: build.sh <bin-dir> [<path to #363's gvproxy-sealed.patch>]     Needs Go and git.
set -e
H=$(cd $(dirname $0); pwd); B=$(mkdir -p $1; cd $1; pwd); P363=$2
UPSTREAM=197eea05   # containers/gvisor-tap-vsock main on 25 November 2026, v0.9.0 and 52 commits
S=$(mktemp -d); git clone -q https://github.com/containers/gvisor-tap-vsock.git $S/gv; cd $S/gv; git checkout -q $UPSTREAM
go build -o $B/gvproxy-stock ./cmd/gvproxy; go build -o $B/gvforwarder ./cmd/vm
if [ -n "$P363" ]; then git apply $P363; go build -o $B/gvproxy-363 ./cmd/gvproxy; git checkout -q .; fi
git apply $H/gvproxy-silo.patch; go build -o $B/gvproxy-silo ./cmd/gvproxy
go test ./pkg/services/forwarder/ ./pkg/services/dns/ ./cmd/gvproxy/
(cd $H/bench && go build -o $B/fwd .); (cd $H/bench/fwdgram && cp ../go.sum . && go build -o $B/fwdgram . && rm go.sum)
(cd $H/boundary && go build -o $B/standin-boundary .)
rm -rf $S; ls $B
