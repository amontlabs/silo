#!/bin/bash
# Test 19 on Linux: its summary over the reader's info of dissect.apfs's disks.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_apfs_data || { echo "skip: no network for the test disks"; exit 0; }
disk_run 19 tests/19-reader-gaps.sh
grep -q '"key":"all-seen"' "$KIT_OUT" || { echo "FAIL: expected all-seen"; grep outcome "$KIT_OUT"; exit 1; }
disk_expect_outcome
