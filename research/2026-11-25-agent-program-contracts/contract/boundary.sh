#!/bin/bash
# Builds the boundary under test: #374's, as its draft holds it (pull request 374 of amontlabs/silo),
# in .build/. Needs Zig 0.15.2, OpenSSL, zlib and zstd headers.
set -eu
C=$(cd $(dirname $0); pwd); R=$(git -C $C rev-parse --show-toplevel); B=$C/.build
REF=${BOUNDARY_REF:-pull/374/head}
mkdir -p $B
git -C $R fetch -q origin $REF
git -C $R archive FETCH_HEAD research/2026-11-24-spend-budgets/boundary research/2026-11-24-spend-budgets/prices.json | tar x -C $B
(cd $B/research/2026-11-24-spend-budgets/boundary && zig build -Doptimize=ReleaseSafe)
echo "$(git -C $R rev-parse FETCH_HEAD)" > $B/boundary-commit.txt
ls $B/research/2026-11-24-spend-budgets/boundary/zig-out/bin/boundary
