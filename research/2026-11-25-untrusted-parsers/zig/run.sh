#!/bin/bash
# Builds each Zig harness against the drafts' code and runs it under libFuzzer, coverage-guided.
# usage: run.sh <dir from fetch.sh> <seconds per harness>
# Needs Zig 0.15.2, clang 18 with its libFuzzer (libclang-rt-18-dev), zlib.
#
# Zig's -ffuzz instruments the code (inline 8-bit counters in the section __sancov_cntrs) but does
# not register the counters the way clang does; sancov_init.c does it, so libFuzzer's own engine
# drives Zig code. Each harness is built in a copy of the sources it tests.
set -e
H=$(cd $(dirname $0); pwd); S=$(cd $1; pwd); T=${2:-600}; W=${W:-/tmp/silo-fuzz}; mkdir -p $W
clang -c -O2 $H/sancov_init.c -o $W/sancov_init.o
build() { # <name> <harness> <source dir> [link args]
  local n=$1 h=$2 src=$3; shift 3
  rm -rf $W/$n; mkdir -p $W/$n/corpus $W/$n/found; cp $src/*.zig $W/$n/; cp $H/$h $W/$n/
  (cd $W/$n && zig build-obj -ffuzz -fno-stack-check -OReleaseSafe -lc $h -femit-bin=$n.o && clang -fsanitize=fuzzer $n.o $W/sancov_init.o -o fuzz "$@")
}
run() { (cd $W/$1 && ./fuzz -dict=$H/json.dict -max_total_time=$T -timeout=10 -rss_limit_mb=2048 -artifact_prefix=found/ corpus 2>&1 | grep -E "DONE|ERROR|SUMMARY|Test unit" | tail -3) | sed "s/^/$1: /"; }
B=$S/385/research/2026-11-24-spend-budgets/boundary/src
build stream h_stream.zig $B -lz; build usage h_usage.zig $B -lz; build ws h_ws.zig $B -lz; build req h_req.zig $B -lz
build record h_record.zig $S/386/research/2026-11-25-record-format/proto/src
# One well-formed line for verify to start from: a control fact that removes a blob and names another.
printf '\x06%s' '2026-11-25T02:41:07.123456789Z","computer":"c1","is":"fact","kind":"control","source":{"id":"s"},"author":{"by":"you","person":"p"},"what":"bulky-removed","blobs":["sha256:5aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"x":"sha256:1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}' > $W/record/corpus/seed
for n in stream usage ws req record; do run $n & done; wait
