#!/bin/bash
# Test 15 on Linux: the guest's work script and write-wait probe, three steps, no computer.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_run 15 tests/15-real-guest-under-work.sh
disk_expect_measure longest_probe_wait_s
disk_expect_outcome
