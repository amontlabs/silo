#!/bin/bash
# Test 04 on Linux: the guest's writers (guest/seqwriter.pl) and the checks (py/04-verify.py)
# against reflink copies taken on XFS while they write.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_run 04 tests/04-clone-moment-in-time.sh
grep -q '"key":"all-hold"' "$KIT_OUT" || { echo "FAIL: expected all-hold on XFS"; grep -E 'outcome|fact' "$KIT_OUT"; exit 1; }
disk_expect_outcome
