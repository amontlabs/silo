#!/bin/bash
# Test 05 on Linux: FIEMAP on XFS reflink copies stands in for F_LOG2PHYS_EXT on APFS clones.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_run 05 tests/05-log2phys-diff.sh
disk_expect_measure written_missed
grep -q '"key":"map-tells"' "$KIT_OUT" || { echo "FAIL: expected map-tells on XFS"; grep outcome "$KIT_OUT"; exit 1; }
disk_expect_outcome
