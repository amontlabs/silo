#!/bin/bash
# Test 51's stand-in drive on Linux: flushed blocks survive a cut, cached ones survive only in
# part, nothing is answered after it, and a flush that does nothing is caught.
set -u
K="$(cd "$(dirname "$0")/.." && pwd)"
python3 -I "$K/py/cutdisk.py" selfcheck | tail -1 | grep -qx PASS || { echo "cutdisk selfcheck failed"; exit 1; }
CUTDISK_MUTANT=flush-noop python3 -I "$K/py/cutdisk.py" selfcheck | tail -1 | grep -qx FAIL || { echo "the planted bug was not caught"; exit 1; }
echo "test 51's stand-in drive: ok"
