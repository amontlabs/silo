#!/bin/bash
# Test 06 on Linux: fsync costs on the work folder's filesystem, the space test on loop-mounted XFS.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_run 06 tests/06-fsync-cost.sh
disk_expect_measure writer_full_128B_x1_prealloc_p99
disk_expect_measure overwrite_after_clone_new_space_mb
disk_expect_outcome
