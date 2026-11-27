#!/bin/bash
# usage: run.sh <work dir>   (Linux, as root, Zig 0.15.2, OpenSSL, zlib, zstd, Python 3, Claude Code)
# Copies #374's boundary and harness (research/2026-11-24-spend-budgets) into <work dir>, applies
# this folder's patches, builds, runs five Claude Codes on five computers set three ways, and
# checks the result. Every key and token is a fake; the providers are #374's stand-ins.
set -eu
H=$(cd $(dirname $0); pwd); S=$H/../2026-11-24-spend-budgets; W=$(realpath -m $1)
rm -rf $W; mkdir -p $W; cp -r $S/boundary $S/run $S/fake $S/prices.json $W/
(cd $W && patch -s -p1 -d boundary < $H/boundary.patch && patch -s run/progs.sh < $H/progs.patch)
(cd $W/boundary && ${ZIG:-zig} build && ${ZIG:-zig} build test)
sed "s#\"prices.json\"#\"$W/prices.json\"#" $H/keyplan.json > $W/run/configs/keyplan.json
bash $W/run/tls.sh > /dev/null
bash $W/run/run.sh keyplan $W/run/configs/keyplan.json $W/run/scenarios/short.json 120 kk kp pp pk rp
python3 $H/check.py $W/out/keyplan
