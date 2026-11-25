#!/bin/bash
# id: 19
# title: What the APFS reader meets inside a real computer: raw writes, snapshots, a new volume, FileVault
# tier: guided
# time: about 60 minutes; you may be asked to turn on FileVault in the computer's window
# needs: the base computer of test 00 (raw disk), 40 GB free, no sudo on the Mac (sudo runs inside the computer only)
# decides: research 11 "Where the trust comes from" and "What the reader has to handle", file-changes-from-outside, macos-ships-facts-first
# question: Can root inside a macOS computer write its raw disk with SIP on, can FileVault be turned on inside, and what does the reader see on clones after 100 local snapshots, an added volume and FileVault?
# outcome raw-writes-allowed: Root inside wrote its own raw disk with SIP on: a write past APFS is possible from main, so the reader's trust must come from the Mac's block map (test 05), never from APFS's rules alone.
# outcome reader-fails: The reader failed on one of the cases (named in the sentence): Silo's reader must handle it; a test disk of that case goes in its tests.
# outcome all-seen: Raw writes were refused with SIP on, and the reader listed the 100 snapshots and the new volume and reported FileVault's volume as a gap (or FileVault could not be turned on, as said): research 11's list of cases holds.
# outcome inconclusive: A step did not run; look at the facts and 19-reader.jsonl.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
READER="$KIT_ROOT/py/apfs/apfsread.py"
SNAPS="${KIT_19_SNAPSHOTS:-100}"
OUT="$KIT_WORK/19-reader.jsonl"; : > "$OUT"

summarize() { # summarize RAW_RESULT FILEVAULT_RESULT: KEY|sentence from OUT
  python3 - "$OUT" "$1" "$2" "$SNAPS" <<'EOF'
import json, sys
rows = {}
for l in open(sys.argv[1]):
    if l.strip():
        r = json.loads(l); rows[r.get("label")] = r
raw, fv, nsnap = sys.argv[2], sys.argv[3], int(sys.argv[4])
def vols(r):
    return [v for c in r.get("containers", []) for v in c.get("volumes", [])]
def data(r):
    for v in vols(r):
        if v.get("role") == "data": return v
    return None
fails, seen = [], []
for label in ("base", "snapshots", "volume", "filevault"):
    r = rows.get(label)
    if r is None: continue
    for c in r.get("containers", []):
        if c.get("gap"): fails.append("%s: container %s: %s" % (label, c.get("partition"), c["gap"]))
    d = data(r)
    if d is None: fails.append("%s: no data volume" % label); continue
    if label == "base" and d.get("encrypted"): seen.append("the data volume was already encrypted before FileVault (%s)" % d.get("gap", "read"))
    if label == "snapshots":
        n = d.get("snapshots_listed")
        if n is None or n < min(nsnap, d.get("snapshots", 0)): fails.append("snapshots: listed %s of %s (%s)" % (n, d.get("snapshots"), d.get("snapshots_gap", "")))
        else: seen.append("%d snapshots listed" % n)
    if label == "volume":
        if any(v.get("name") == "SiloKitExtra" for v in vols(r)): seen.append("the added volume listed")
        else: fails.append("volume: the added volume is not listed")
    if label == "filevault":
        if d.get("encrypted") and d.get("gap"): seen.append("FileVault's data volume a gap (%s)" % d["gap"][:50])
        elif d.get("encrypted"): fails.append("filevault: encrypted but no gap")
        else: seen.append("data volume not encrypted after FileVault (%s)" % fv)
s = "raw writes with SIP on: %s; FileVault: %s; reader: %s" % (raw, fv, "; ".join(seen) or "nothing seen")
if fails: print("reader-fails|" + s + "; failed: " + "; ".join(fails))
elif raw.startswith("allowed"): print("raw-writes-allowed|" + s)
elif not rows or raw == "not tried": print("inconclusive|" + s)
else: print("all-seen|" + s)
EOF
}

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  D="${KIT_APFS_DATA:-${TMPDIR:-/tmp}/silo-kit-apfs-data}"
  [ -s "$D/snapshot.bin" ] && [ -s "$D/encrypted.bin" ] || kit_skip "no APFS test disks in $D (linux/test-apfs.sh fetches them)"
  kit_note "Linux analogue: the summary over dissect.apfs's disks; no computer, no raw writes"
  # Their volumes have no data role; mark the first one as data so the summary reads them.
  for x in "base snapshot" "snapshots snapshot" "filevault encrypted"; do
    # shellcheck disable=SC2086
    set -- $x
    python3 "$READER" info "$D/$2.bin" | python3 -c '
import json, sys
r = json.load(sys.stdin); r["label"] = sys.argv[1]
r["containers"][0]["volumes"][0]["role"] = "data"
if sys.argv[1] == "snapshots": r["containers"][0]["volumes"].append({"name": "SiloKitExtra"})
print(json.dumps(r))' "$1" >> "$OUT"
  done
  python3 -c 'import json,sys; r=json.loads(open(sys.argv[1]).readlines()[1]); r["label"]="volume"; print(json.dumps(r))' "$OUT" >> "$OUT"
  r="$(summarize "refused (analogue)" "on (analogue)")"; kit_outcome "${r%%|*}" "${r#*|}"; exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 40
[ -x "$BIN/clonebench" ] || kit_skip "the kit's programs are not built: run ./kit build"
fmt="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("disk_format","raw"))' "$KIT_BASE_VM/config.json")"
[ "$fmt" = raw ] || kit_skip "the base computer's disk is $fmt; the reader reads raw images only"
kit_confirm "Test 19 starts a computer and, inside it only, tries raw disk writes, makes $SNAPS local snapshots, adds a volume and turns on FileVault, about 60 minutes."

B="$(vm_fresh t19)" || exit 1  # vm_fresh already recorded why
trap 'vm_destroy "$B"; rm -f "$KIT_WORK"/ck-*.img; vm_ssh "$B" "$VM_USER" "rm -f /tmp/fv.plist" 2>/dev/null' EXIT
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH"; }
gsh() { vm_ssh "$B" "$VM_USER" "$@"; }
gsudo() { vm_sudo "$B" "$VM_USER" "$@"; }

read_clone() { # read_clone LABEL: settle, clone the disk, run the reader's info on it
  gsh 'sync; sleep 3; sync' >/dev/null 2>&1
  rm -f "$KIT_WORK/ck-$1.img"
  "$BIN/clonebench" clone "$B/disk.img" "$KIT_WORK/ck-$1.img" >/dev/null || kit_fail "clone failed"
  python3 "$READER" info "$KIT_WORK/ck-$1.img" 2> "$KIT_WORK/err-$1.txt" \
    | python3 -c 'import json,sys; r=json.load(sys.stdin); r["label"]=sys.argv[1]; print(json.dumps(r))' "$1" >> "$OUT" \
    || { kit_attach "$KIT_WORK/err-$1.txt"; kit_note "the reader crashed on $1 (a bug of the probe)"; }
}

read_clone base
kit_fact guest_sip "$(gsh 'csrutil status' 2>&1 | head -1)"
kit_fact guest_layout "$(gsh 'diskutil list' 2>&1 | tr -s ' ' | head -40 | tr '\n' ';')"

# 1. Raw writes with SIP on: the first block read and written back unchanged, on the whole disk
#    (the protective MBR and GPT header, which nothing writes while it runs) and on the APFS
#    physical store of the boot volume (its block 0, the superblock copy APFS rarely writes).
#    Nothing changes; only whether the write is allowed.
store="$(gsh "diskutil info / | awk -F': *' '/APFS Physical Store/{print \$2; exit}'" | tr -d ' \r')"
whole="$(echo "$store" | sed 's/s[0-9]*$//')"
raw="not tried"
if [ -n "$store" ]; then
  raw="refused"
  for dev in "/dev/r$whole" "/dev/r$store"; do
    r="$(gsudo "dd if=$dev of=/tmp/blk bs=4096 count=1 2>&1 && dd if=/tmp/blk of=$dev bs=4096 count=1 conv=notrunc 2>&1; echo rc=\$?" 2>&1 | tr '\n' ' ')"
    kit_fact "raw_write_${dev#/dev/}" "$r"
    case "$r" in *rc=0*) raw="allowed on $dev" ;; esac
  done
fi
kit_fact raw_writes "$raw"

# 2. Local snapshots.
gsudo "i=0; while [ \$i -lt $SNAPS ]; do tmutil localsnapshot / >/dev/null 2>&1 || echo fail; sleep 1.2; i=\$((i+1)); done" > "$KIT_WORK/snap.txt" 2>&1
kit_measure snapshot_failures "$(grep -c fail "$KIT_WORK/snap.txt")" count "of $SNAPS tmutil localsnapshot"
kit_measure local_snapshots "$(gsh 'tmutil listlocalsnapshots / 2>/dev/null' | grep -c com.apple)" count "as tmutil lists them"
read_clone snapshots

# 3. A volume added to the boot container, with a file in it.
cont="$(gsh "diskutil info / | awk -F': *' '/APFS Container:/{print \$2; exit}'" | tr -d ' \r')"
kit_fact add_volume "$(gsudo "diskutil apfs addVolume $cont APFS SiloKitExtra && echo hello > /Volumes/SiloKitExtra/hello.txt && echo added" 2>&1 | tail -1)"
read_clone volume

# 4. FileVault inside, with the provisioned user's password (Apple's fdesetup -inputplist).
python3 - "$VM_USER" "$(vm_password)" <<'EOF' | gsh 'umask 077; cat > /tmp/fv.plist'
import plistlib, sys
sys.stdout.buffer.write(plistlib.dumps({"Username": sys.argv[1], "Password": sys.argv[2]}))
EOF
fvout="$(gsudo 'fdesetup enable -inputplist < /tmp/fv.plist > /dev/null 2>/tmp/fv.err; echo rc=$?; cat /tmp/fv.err; rm -f /tmp/fv.plist /tmp/fv.err' 2>&1 | tr '\n' ' ')"
kit_fact fdesetup_enable "$fvout"
case "$fvout" in
  *rc=0*) ;;
  *)
    vm_stop "$B" 60 >/dev/null
    vm_start "$B" --gui >/dev/null
    vm_wait_ssh "$B" "$VM_USER" 600 >/dev/null || kit_fail "the computer did not come back"
    kit_human "In the computer's window, log in as $VM_USER (password in ~/SiloKit/ssh/password), open System Settings > Privacy & Security > FileVault, and turn it on (say no to iCloud; any recovery key). fdesetup said: $fvout"
    ;;
esac
i=0; st=""
while [ "$i" -lt 60 ]; do
  st="$(gsh 'fdesetup status' 2>&1 | tr '\n' ' ')"
  case "$st" in *"is On"*) case "$st" in *progress*) ;; *) break ;; esac ;; *"is Off"*) [ "$i" -ge 3 ] && break ;; esac
  sleep 15; i=$((i + 1))
done
kit_fact fdesetup_status "$st"
case "$st" in *"is On"*) fv="on" ;; *) fv="could not be turned on ($st)" ;; esac
read_clone filevault
kit_attach "$OUT"
r="$(summarize "$raw" "$fv")"
kit_outcome "${r%%|*}" "${r#*|}"
