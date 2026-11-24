#!/bin/bash
# id: 17
# title: How often a clone of a busy computer's disk has a broken newest APFS checkpoint
# tier: long
# time: about 45 minutes
# needs: the base computer of test 00 (raw disk), 40 GB free, no sudo on the Mac
# decides: research 11 "What the reader has to handle" (the fallback), file-changes-from-outside
# question: Of 100 clones of a macOS computer's disk taken while programs inside write, how many have a newest APFS checkpoint that does not check, and does an older one always open?
# outcome never-broken: No clone had a broken newest checkpoint: the reader's fallback is a rare path; it stays, tested on dissect.apfs's corrupt image.
# outcome sometimes-broken: Some clones had a broken newest checkpoint and an older one opened every time: the fallback is a normal path, the record says which checkpoint it read and that a newer one was skipped.
# outcome unreadable: Some clone had no checkpoint that opens in some container: the reader would give a gap for that checkpoint; Silo needs a pause (test 04) or a retry of the clone before reading.
# outcome inconclusive: The computer or the probe failed; look at 17-checks.jsonl.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
N="${KIT_17_CLONES:-100}"
READER="$KIT_ROOT/py/apfs/apfsread.py"
OUT="$KIT_WORK/17-checks.jsonl"; : > "$OUT"

summarize() { # reads OUT, prints KEY|sentence
  python3 - "$OUT" <<'EOF'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
rows = [r for r in rows if r.get("label", "busy").startswith("busy")]
n = len(rows); broken = 0; unread = 0; why = {}; gaps = {}
for r in rows:
    cs = r.get("containers", [])
    if any(c.get("gap") for c in cs) or not cs:
        unread += 1
        for c in cs:
            if c.get("gap"): gaps[c["gap"][:60]] = gaps.get(c["gap"][:60], 0) + 1
    if any(c.get("newest_ok") is False for c in cs):
        broken += 1
        for c in cs:
            for s in c.get("skipped") or []:
                k = s["why"].split(" at ")[0][:50]; why[k] = why.get(k, 0) + 1
s = "%d of %d clones had a broken newest checkpoint in some container, %d had a container that did not open" % (broken, n, unread)
if why: s += "; why: " + ", ".join("%s (%d)" % kv for kv in sorted(why.items(), key=lambda x: -x[1])[:3])
if gaps: s += "; gaps: " + ", ".join("%s (%d)" % kv for kv in sorted(gaps.items(), key=lambda x: -x[1])[:3])
if n == 0: print("inconclusive|no clone was checked")
elif unread: print("unreadable|" + s)
elif broken: print("sometimes-broken|" + s)
else: print("never-broken|" + s)
EOF
}

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: the reader on disks Apple's APFS wrote (dissect.apfs's), when linux/test-apfs.sh
  # has fetched them into KIT_APFS_DATA; the corrupt one has a broken newest checkpoint.
  D="${KIT_APFS_DATA:-${TMPDIR:-/tmp}/silo-kit-apfs-data}"
  [ -s "$D/corrupt.bin" ] && [ -s "$D/snapshot.bin" ] || kit_skip "no APFS test disks in $D (linux/test-apfs.sh fetches them)"
  kit_note "Linux analogue: the reader on dissect.apfs's test disks, not on clones of a computer"
  for f in snapshot corrupt snapshot; do python3 "$READER" checkpoint "$D/$f.bin" >> "$OUT" || kit_fail "reader crashed on $f"; done
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
kit_confirm "Test 17 starts a computer, keeps it writing and clones its disk $N times, about 45 minutes."

B="$(vm_fresh t17)" || exit 1  # vm_fresh already recorded why
trap 'vm_destroy "$B"; rm -f "$KIT_WORK/c.img"' EXIT
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH"; }
kit_measure first_start_to_ssh_s "$s" s
vm_ssh "$B" "$VM_USER" 'mkdir -p ~/sk && cat > ~/sk/churn.pl' < "$KIT_ROOT/guest/churn.pl"

check_clone() { # check_clone LABEL: clone the running disk, check its checkpoints, delete it
  rm -f "$KIT_WORK/c.img"
  local o; o="$("$BIN/clonebench" clone "$B/disk.img" "$KIT_WORK/c.img")" || kit_fail "clone failed"
  python3 "$READER" checkpoint "$KIT_WORK/c.img" > "$KIT_WORK/one.json" 2> "$KIT_WORK/one.err" \
    || { kit_attach "$KIT_WORK/one.err"; kit_fail "the reader crashed (a bug of the probe, see one.err)"; }
  python3 - "$KIT_WORK/one.json" "$1" "$o" >> "$OUT" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1])); r["label"] = sys.argv[2]; r["clone"] = json.loads(sys.argv[3])
print(json.dumps(r))
EOF
  rm -f "$KIT_WORK/c.img"
}

check_clone idle
vm_ssh "$B" "$VM_USER" '(nohup perl ~/sk/churn.pl ~/churn 200 </dev/null >/dev/null 2>&1 &); (nohup sh -c "while :; do dd if=/dev/urandom of=\$HOME/big.bin bs=1m count=512 2>/dev/null; done" </dev/null >/dev/null 2>&1 &)'
sleep 10
i=1
while [ "$i" -le "$N" ]; do
  sleep "$(python3 -c 'import random; print("%.2f" % random.uniform(0.3, 3))')"
  check_clone "busy-$i"
  [ $((i % 10)) = 0 ] && kit_log "$i of $N clones checked"
  vm_running "$B" || kit_fail "the computer stopped during the clones"
  i=$((i + 1))
done
vm_sudo "$B" "$VM_USER" 'pkill -f churn.pl; pkill -f "dd if=/dev/urandom"' >/dev/null 2>&1
kit_attach "$OUT"
r="$(summarize)"
kit_outcome "${r%%|*}" "${r#*|}"
