#!/bin/bash
# id: 18
# title: Leaf-address diff of two clones of a computer with many files (probe only)
# tier: long
# time: about 50 minutes
# needs: the base computer of test 00 (raw disk), 40 GB free, no sudo on the Mac
# decides: file-changes-from-outside ("the time follows the size of the tree"), research 11's estimate of a comparison's cost
# question: On clones of a real macOS computer's disk with many files, does the leaf-address diff give the same changes as a full walk, and do the blocks it reads follow the changes rather than the tree?
# outcome follows-changes: The leaf diff matched the full walk every time and read a small part of what the walk read for small changes: research 11's design holds on a real disk; the Python probe's times bound nothing for Silo's reader, which does not exist yet.
# outcome always-wide: The leaf diff matched but read nearly as much as the full walk even for 10 changed files (macOS changes leaves across the tree by itself): the cost follows the tree after all; keep research 11's estimate out of targets until Silo's reader is measured.
# outcome wrong: Some leaf diff differed from the full walk on the same pair: the leaf-address method misses changes on a real disk; research 11's design must change before the reader is built.
# outcome no-read: The probe could not read the clones' data volume (a gap): what it failed on is in the sentence; Silo's reader has to handle it.
# outcome inconclusive: The computer or the probe failed; look at 18-diffs.jsonl.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
NFILES="${KIT_18_FILES:-200000}"
CHANGES="${KIT_18_CHANGES:-10 1000 50000 all}"
READER="$KIT_ROOT/py/apfs/apfsread.py"
OUT="$KIT_WORK/18-diffs.jsonl"; : > "$OUT"

summarize() {
  python3 - "$OUT" <<'EOF'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
if not rows: print("inconclusive|no diff ran"); sys.exit()
gaps = [r for r in rows if r.get("gap")]
if gaps: print("no-read|the probe gave a gap: %s" % gaps[0]["gap"]); sys.exit()
wrong = [r for r in rows if r.get("same_as_full") is False]
parts = []
for r in rows:
    parts.append("%s files changed: %d records differ, leaf diff read %d blocks in %.1f s, full walk %d in %.1f s"
                 % (r.get("label"), r["records_differ"], r["leaf_blocks_read"], r["leaf_seconds"],
                    r.get("full_blocks_read", 0), r.get("full_seconds", 0)))
s = "; ".join(parts) + " (Python probe; Silo's reader does not exist yet)"
small = [r for r in rows if r.get("label") not in ("all",)]
ratio = min((r["leaf_blocks_read"] / float(max(r.get("full_blocks_read", 1), 1)) for r in small), default=1)
if wrong: print("wrong|%d of %d diffs differ from the full walk; %s" % (len(wrong), len(rows), s))
elif ratio > 0.5: print("always-wide|" + s)
else: print("follows-changes|" + s)
EOF
}

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  D="${KIT_APFS_DATA:-${TMPDIR:-/tmp}/silo-kit-apfs-data}"
  [ -s "$D/snapshot.bin" ] || kit_skip "no APFS test disks in $D (linux/test-apfs.sh fetches them)"
  kit_note "Linux analogue: snapshots of dissect.apfs's disk stand in for clones (1,027 files)"
  for p in "#510 #511 1" "#400 #511 111" "#0 #511 1027"; do
    # shellcheck disable=SC2086
    set -- $p
    python3 "$READER" leafdiff "$D/snapshot.bin@$1" "$D/snapshot.bin@$2" --full \
      | python3 -c 'import json,sys; r=json.load(sys.stdin); r["label"]=sys.argv[1]; print(json.dumps(r))' "$3" >> "$OUT"
  done
  kit_attach "$OUT"
  r="$(summarize)"; kit_outcome "${r%%|*}" "${r#*|}"; exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 40
[ -x "$BIN/clonebench" ] || kit_skip "the kit's programs are not built: run ./kit build"
fmt="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("disk_format","raw"))' "$KIT_BASE_VM/config.json")"
[ "$fmt" = raw ] || kit_skip "the base computer's disk is $fmt; the reader reads raw images only"
kit_confirm "Test 18 starts a computer, makes $NFILES files in it and diffs clones of its disk, about 50 minutes."

B="$(vm_fresh t18)" || exit 1  # vm_fresh already recorded why
trap 'vm_destroy "$B"; rm -f "$KIT_WORK"/ck-*.img' EXIT
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH"; }

settle_clone() { # settle_clone NAME: the guest writes its changes out, then the Mac clones the disk
  vm_ssh "$B" "$VM_USER" 'sync; sleep 3; sync' >/dev/null 2>&1
  rm -f "$KIT_WORK/ck-$1.img"
  "$BIN/clonebench" clone "$B/disk.img" "$KIT_WORK/ck-$1.img" >/dev/null || kit_fail "clone failed"
}

t="$(kit_now)"
vm_ssh "$B" "$VM_USER" "perl -e 'use File::Path qw(make_path); for (my \$i = 0; \$i < $NFILES; \$i++) { my \$d = sprintf(\"%s/tree/d%04d\", \$ENV{HOME}, int(\$i / 1000)); make_path(\$d) unless \$i % 1000; open(my \$f, \">\", \"\$d/f\$i.txt\") or die; print \$f \"file \$i\\n\"; close \$f; }'" \
  || kit_fail "making the files failed"
kit_measure make_files_s "$(python3 -c "print('%.1f' % ($(kit_now) - $t))")" s "$NFILES files in the guest"
settle_clone base
python3 "$READER" info "$KIT_WORK/ck-base.img" > "$KIT_WORK/info-base.json" 2>&1 || true
kit_attach "$KIT_WORK/info-base.json"

for m in $CHANGES; do
  if [ "$m" = all ]; then
    vm_ssh "$B" "$VM_USER" 'find ~/tree -type f -exec touch {} +' >/dev/null
  else
    vm_ssh "$B" "$VM_USER" "perl -e 'my \$n = $m; my \$step = int($NFILES / \$n) || 1; for (my \$k = 0, my \$i = 0; \$k < \$n; \$k++, \$i += \$step) { my \$p = sprintf(\"%s/tree/d%04d/f%d.txt\", \$ENV{HOME}, int(\$i / 1000), \$i); open(my \$f, \">>\", \$p) or next; print \$f \"x\"; close \$f; }'"
  fi
  settle_clone "$m"
  kit_log "diff after $m files changed (the full walk takes a while)"
  python3 "$READER" leafdiff "$KIT_WORK/ck-base.img" "$KIT_WORK/ck-$m.img" --full 2> "$KIT_WORK/err-$m.txt" \
    | python3 -c 'import json,sys; r=json.load(sys.stdin); r["label"]=sys.argv[1]; print(json.dumps(r))' "$m" >> "$OUT" \
    || { kit_attach "$KIT_WORK/err-$m.txt"; kit_fail "the reader crashed (a bug of the probe, see err-$m.txt)"; }
  rm -f "$KIT_WORK/ck-$m.img"
done
kit_attach "$OUT"
r="$(summarize)"
kit_outcome "${r%%|*}" "${r#*|}"
