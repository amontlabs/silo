#!/bin/bash
# Test 18 on Linux: its summary over leaf diffs of dissect.apfs's snapshots (needs
# linux/test-apfs.sh to have fetched them).
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_apfs_data || { echo "skip: no network for the test disks"; exit 0; }
disk_run 18 tests/18-leaf-diff-scale.sh
grep -q '"key":"follows-changes"' "$KIT_OUT" || { echo "FAIL: expected follows-changes"; grep outcome "$KIT_OUT"; exit 1; }
disk_expect_outcome
