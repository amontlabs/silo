#!/bin/bash
# Test 39 on Linux: the guest's checksums are what dissect.apfs's case_sensitive disk is known to
# hold, and the reader reads every compressed file of it from outside.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
. "$KIT_ROOT/linux/disk-common.sh"
disk_apfs_data || { echo "skip: no network for the test disks"; exit 0; }
python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import apfscontent as C; sys.exit(0 if C.LZ.fn else 1)' "$KIT_ROOT/py/apfs" ||
  { echo "skip: no LZFSE decoder here (install liblzfse)"; exit 0; }
disk_run 39 tests/39-compressed-files-from-outside.sh
grep -q '"key":"reads-all"' "$KIT_OUT" || { echo "FAIL: expected reads-all"; grep outcome "$KIT_OUT"; exit 1; }
disk_expect_outcome
# Test 39's comparison catches a file read wrong, and a file the guest flags but the reader reads
# as plain.
W="$T/cmp"; mkdir -p "$W"
printf '%s  /a\n%s  /b\n' "$(printf a | shasum -a 256 | cut -c1-64)" "$(printf b | shasum -a 256 | cut -c1-64)" > "$W/g.sha"
printf '{"path": "/a", "size": 1, "compressed": "lzvn-fork", "sha256": "%s"}\n{"path": "/b", "size": 1, "compressed": null, "sha256": "%s"}\n' \
  "$(printf x | shasum -a 256 | cut -c1-64)" "$(printf b | shasum -a 256 | cut -c1-64)" > "$W/r.jsonl"
k="$(python3 "$KIT_ROOT/py/39-compare.py" "$W/g.sha" "$W/r.jsonl" | head -1 | cut -d'|' -f1)"
[ "$k" = reads-wrong ] || { echo "FAIL: a file read wrong gave $k"; exit 1; }
printf '{"path": "/a", "size": 1, "compressed": "lzvn-fork", "sha256": "%s"}\n{"path": "/b", "size": 1, "compressed": null, "sha256": "%s"}\n' \
  "$(printf a | shasum -a 256 | cut -c1-64)" "$(printf b | shasum -a 256 | cut -c1-64)" > "$W/r.jsonl"
echo /b > "$W/f.txt"
k="$(python3 "$KIT_ROOT/py/39-compare.py" "$W/g.sha" "$W/r.jsonl" "$W/f.txt" | head -1 | cut -d'|' -f1)"
[ "$k" = reads-wrong ] || { echo "FAIL: a flagged file read as plain gave $k"; exit 1; }
printf '{"path": "/a", "compressed": "lzbitmap-fork", "gap": "LZBITMAP: no open decoder"}\n{"path": "/b", "size": 1, "compressed": null, "sha256": "%s"}\n' \
  "$(printf b | shasum -a 256 | cut -c1-64)" > "$W/r.jsonl"
k="$(python3 "$KIT_ROOT/py/39-compare.py" "$W/g.sha" "$W/r.jsonl" | head -1 | cut -d'|' -f1)"
[ "$k" = type-unread ] || { echo "FAIL: an LZBITMAP gap gave $k"; exit 1; }
echo "ok   39's comparison: reads-wrong twice, type-unread"
