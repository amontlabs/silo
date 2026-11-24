#!/bin/bash
# Test 17 on Linux: its summary over the reader's checks of dissect.apfs's disks (needs
# linux/test-apfs.sh to have fetched them).
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_apfs_data || { echo "skip: no network for the test disks"; exit 0; }
disk_run 17 tests/17-checkpoint-broken-rate.sh
grep -q '"key":"sometimes-broken"' "$KIT_OUT" || { echo "FAIL: expected sometimes-broken"; grep outcome "$KIT_OUT"; exit 1; }
disk_expect_outcome
