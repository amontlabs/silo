#!/bin/bash
# Test 03 on Linux: the checkpoint loop on a loop-mounted XFS image with reflinks. Needs root.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_run 03 tests/03-apfs-clone-stall.sh
disk_expect_measure raw_ck10_extents
disk_expect_outcome
