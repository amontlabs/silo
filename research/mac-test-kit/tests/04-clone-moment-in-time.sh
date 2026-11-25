#!/bin/bash
# id: 04
# title: Is a clone of a running computer's disk a moment in time, per disk mode
# tier: vm
# time: about 60 minutes (19 starts of a computer)
# needs: the base computer of test 00, 40 GB free, no sudo on the Mac
# step: 2
# decides: checkpoint-per-system (the mode main's disk is attached in; whether a checkpoint needs a pause), whole-computer-grain
# question: When the Mac clones the disk of a running macOS computer, does the clone keep every write the guest flushed before it, with no write missing before a later one, in each of the 9 caching and sync modes, with and without the guest flushing?
# outcome all-hold: In all 9 modes the clone held every flushed write, no hole, and booted: a checkpoint needs no pause; main's disk is attached in whichever mode is fastest (the default, automatic and full, unless test 15 says otherwise).
# outcome some-lose-pause-helps: Some modes lost flushed writes or left holes, and pausing the computer for the clone fixed them: attach main's disk in a mode that held (named in the sentence), or pause for each checkpoint in the others.
# outcome some-lose: Some modes lost flushed writes or left holes, even with a pause: attach main's disk only in a mode that held (named in the sentence).
# outcome none-hold: No mode kept every flushed write: a checkpoint of a running computer is not a moment in time; checkpoint-per-system needs the computer stopped, or another way (research 07).
# outcome inconclusive: A computer did not start or a writer did not run; look at 04-modes.csv and the logs attached.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
[ -x "$BIN/clonebench" ] || kit_skip "the kit's programs are not built: run ./kit build"
CACHES="${KIT_04_CACHES:-automatic cached uncached}"
SYNCS="${KIT_04_SYNCS:-full fsync none}"
RUN_S="${KIT_04_RUN_S:-6}"          # seconds the writers run before the clone
HALF=524288                          # blocks of each raw writer's half of the extra disk (2 GiB)
SIG="SILOKIT-EXTRA-$$-$(date +%s)"
CSV="$KIT_WORK/04-modes.csv"
echo "cache,sync,pause,clone_s,raw_flushed_acked,raw_flushed_prefix,raw_flushed_after,raw_unflushed_acked,raw_unflushed_prefix,raw_unflushed_after,file_flushed_acked,file_flushed_prefix,file_flushed_bad,file_unflushed_acked,file_unflushed_prefix,file_unflushed_bad,booted,flush_call" > "$CSV"
verify() { python3 "$KIT_ROOT/py/04-verify.py" "$@"; }
jget() { python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get(sys.argv[2], ""))' "$1" "$2"; }

make_extra() { # a sparse raw disk whose first block holds the signature
  python3 - "$1" "$SIG" $(((2 * HALF + 1) * 4096)) <<'EOF'
import sys
with open(sys.argv[1], "wb") as f:
    f.truncate(int(sys.argv[3])); f.write(sys.argv[2].encode().ljust(4096, b"\0"))
EOF
}

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: the guest's writers run here, against an image file and a folder on XFS, and
  # the "clone" is a reflink copy taken while they write. No computer, no modes.
  [ "$(id -u)" = 0 ] && command -v mkfs.xfs >/dev/null 2>&1 || kit_skip "Linux analogue needs root and mkfs.xfs"
  command -v perl >/dev/null 2>&1 || kit_skip "needs perl"
  M="$KIT_WORK/mnt"; mkdir -p "$M"
  truncate -s 6G "$KIT_WORK/xfs.img"; mkfs.xfs -q -f -m reflink=1 "$KIT_WORK/xfs.img" || kit_fail mkfs.xfs
  mount -o loop "$KIT_WORK/xfs.img" "$M" || kit_skip "cannot mount a loop device here"
  PIDS=""
  trap 'for p in $PIDS; do kill "$p" 2>/dev/null; done; sleep 0.5; umount "$M"; rm -f "$KIT_WORK/xfs.img"' EXIT
  kit_note "Linux analogue on XFS: checks the writers and the checks, says nothing of the Mac's modes"
  E="$M/extra.img"; make_extra "$E"; mkdir -p "$M/sk"
  W="$KIT_ROOT/guest/seqwriter.pl"
  perl "$W" raw "$E" 5110a001 1 "$M/sk/c-r1" 1 "$HALF" "$HALF" & PIDS="$PIDS $!"
  perl "$W" raw "$E" 5110a000 0 "$M/sk/c-r0" $((1 + HALF)) "$HALF" "$HALF" & PIDS="$PIDS $!"
  perl "$W" file "$M/sk/flushed.log" 5110f001 1 "$M/sk/c-f1" & PIDS="$PIDS $!"
  perl "$W" file "$M/sk/unflushed.log" 5110f000 0 "$M/sk/c-f0" & PIDS="$PIDS $!"
  sleep 3
  before="$(for f in c-r1 c-r0 c-f1 c-f0; do tr -d ' ' < "$M/sk/$f"; echo; done | tr '\n' ' ')"
  t0="$(kit_now)"
  cp --reflink=always "$E" "$M/ck-extra.img"; mkdir -p "$M/ck"; cp --reflink=always "$M/sk/flushed.log" "$M/sk/unflushed.log" "$M/ck/"
  cs="$(python3 -c "print('%.3f' % ($(kit_now) - $t0))")"
  for p in $PIDS; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; PIDS=""
  # shellcheck disable=SC2086
  set -- $before
  r1="$(verify raw "$M/ck-extra.img" 5110a001 1 "$HALF" $(($1 + 20000)))"
  r0="$(verify raw "$M/ck-extra.img" 5110a000 $((1 + HALF)) "$HALF" $(($2 + 20000)))"
  f1="$(verify file "$M/ck/flushed.log" 5110f001)"; f0="$(verify file "$M/ck/unflushed.log" 5110f000)"
  kit_fact raw_flushed "$r1"; kit_fact raw_unflushed "$r0"; kit_fact file_flushed "$f1"; kit_fact file_unflushed "$f0"
  kit_measure clone_s "$cs" s "reflink copies while four writers run"
  ok=1
  [ "$(jget "$r1" prefix)" -ge "$1" ] && [ "$(jget "$r1" after_prefix)" = 0 ] || ok=0
  [ "$(jget "$f1" prefix)" -ge "$3" ] && [ "$(jget "$f1" bad_after_prefix)" = 0 ] || ok=0
  if [ "$ok" = 1 ]; then
    kit_outcome all-hold "Linux analogue: the reflink copy held all $1 flushed raw writes acknowledged before it (prefix $(jget "$r1" prefix)) and $3 flushed records (prefix $(jget "$f1" prefix)), no holes"
  else
    kit_outcome none-hold "Linux analogue: the copy lost flushed writes: raw $r1 (acked $1), file $f1 (acked $3)"
  fi
  exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 40
kit_confirm "Test 04 starts a macOS computer 19 times or more (one per disk mode, one per clone), about 60 minutes."

B="$(vm_fresh t04)" || exit 1  # vm_fresh already recorded why
CK="$KIT_WORK/ck.silovm"
E="$KIT_WORK/extra.img"
trap 'vm_destroy "$CK" 2>/dev/null; vm_destroy "$B"; rm -f "$E" "$KIT_WORK/ck-extra.img"' EXIT

# First start: provisioning, then stop. Every mode below starts the same computer again.
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH after its first start"; }
kit_measure first_start_to_ssh_s "$s" s
vm_ssh "$B" "$VM_USER" 'command -v perl && perl -v | sed -n 2p' > "$KIT_WORK/perl.txt" 2>&1 || true
grep -q perl "$KIT_WORK/perl.txt" || { kit_attach "$KIT_WORK/perl.txt"; kit_fail "no perl in the guest: the writers need it"; }
kit_fact guest_perl "$(tail -1 "$KIT_WORK/perl.txt")"
vm_stop "$B" 60 >/dev/null

ck_bundle() { # ck_bundle SRC DST: the checkpoint of a running computer, as Silo would take it
  rm -rf "$2"; mkdir -p "$2"
  local o; o="$("$BIN/clonebench" clone "$1/disk.img" "$2/disk.img")" || return 1
  for f in aux.img hardware-model.bin machine-id.bin config.json; do cp -c "$1/$f" "$2/$f" || return 1; done
  jget "$o" seconds
}

run_mode() { # run_mode CACHE SYNC PAUSE(0|1)
  local c="$1" s="$2" p="$3" tag="$1-$2-pause$3" dev before after cs booted=no how
  make_extra "$E"
  vm_start "$B" --disk-cache "$c" --disk-sync "$s" --extra-disk "$E" >/dev/null
  if ! vm_wait_ssh "$B" "$VM_USER" 600 >/dev/null; then
    cp "$B/run.log" "$KIT_WORK/run-$tag.log"; kit_attach "$KIT_WORK/run-$tag.log"
    kit_note "$tag: the computer did not answer SSH"; vm_stop "$B" 30 >/dev/null
    echo "$c,$s,$p,,,,,,,,,,,,,,start-failed," >> "$CSV"; return
  fi
  vm_ssh "$B" "$VM_USER" 'rm -rf ~/sk && mkdir -p ~/sk && cat > ~/sk/seqwriter.pl' < "$KIT_ROOT/guest/seqwriter.pl"
  vm_ssh "$B" "$VM_USER" 'cat > ~/sk/finddisk.sh' < "$KIT_ROOT/guest/finddisk.sh"
  dev="$(vm_sudo "$B" "$VM_USER" "sh /Users/$VM_USER/sk/finddisk.sh $SIG")"
  if [ -z "$dev" ]; then
    kit_note "$tag: the extra disk was not found in the guest"; vm_stop "$B" 30 >/dev/null
    echo "$c,$s,$p,,,,,,,,,,,,,,no-extra-disk," >> "$CSV"; return
  fi
  vm_sudo "$B" "$VM_USER" "nohup perl /Users/$VM_USER/sk/seqwriter.pl raw $dev 5110a001 1 /Users/$VM_USER/sk/c-r1 1 $HALF $HALF </dev/null >/dev/null 2>&1 &"
  vm_sudo "$B" "$VM_USER" "nohup perl /Users/$VM_USER/sk/seqwriter.pl raw $dev 5110a000 0 /Users/$VM_USER/sk/c-r0 $((1 + HALF)) $HALF $HALF </dev/null >/dev/null 2>&1 &"
  vm_ssh "$B" "$VM_USER" 'cd ~/sk && (nohup perl seqwriter.pl file flushed.log 5110f001 1 c-f1 </dev/null >/dev/null 2>&1 &) && (nohup perl seqwriter.pl file unflushed.log 5110f000 0 c-f0 </dev/null >/dev/null 2>&1 &)'
  sleep "$RUN_S"
  counts() { vm_ssh "$B" "$VM_USER" 'for f in c-r1 c-r0 c-f1 c-f0; do tr -d " " < ~/sk/$f 2>/dev/null || printf 0; echo; done' | tr '\n' ' '; }
  before="$(counts)"
  [ "$p" = 1 ] && "$SILOVM" ctl --bundle "$B" pause >/dev/null
  cs="$(ck_bundle "$B" "$CK")" || kit_fail "$tag: the clone failed"
  "$BIN/clonebench" clone "$E" "$KIT_WORK/ck-extra.img" >/dev/null || kit_fail "$tag: the extra disk's clone failed"
  [ "$p" = 1 ] && "$SILOVM" ctl --bundle "$B" resume >/dev/null
  after="$(counts)"
  how="$(vm_ssh "$B" "$VM_USER" 'cat ~/sk/c-r1.how ~/sk/c-f1.how 2>/dev/null' | tr '\n' '/')"
  vm_sudo "$B" "$VM_USER" "pkill -f seqwriter.pl" >/dev/null 2>&1
  vm_stop "$B" 60 >/dev/null
  # shellcheck disable=SC2086
  set -- $before
  local a1="$1" a0="$2" af1="$3" af0="$4"
  # shellcheck disable=SC2086
  set -- $after
  local r1 r0 f1="" f0=""
  r1="$(verify raw "$KIT_WORK/ck-extra.img" 5110a001 1 "$HALF" "$(($1 + 2000 > HALF ? HALF : $1 + 2000))")"
  r0="$(verify raw "$KIT_WORK/ck-extra.img" 5110a000 $((1 + HALF)) "$HALF" "$(($2 + 2000 > HALF ? HALF : $2 + 2000))")"
  rm -f "$KIT_WORK/ck-extra.img"
  # Boot the clone of main's disk and read the two record files in it.
  vm_start "$CK" >/dev/null
  if vm_wait_ssh "$CK" "$VM_USER" 600 >/dev/null; then
    booted=yes
    vm_ssh_pw "$CK" "$VM_USER" 'cat ~/sk/flushed.log' > "$KIT_WORK/f1.log" 2>/dev/null
    vm_ssh_pw "$CK" "$VM_USER" 'cat ~/sk/unflushed.log' > "$KIT_WORK/f0.log" 2>/dev/null
    f1="$(verify file "$KIT_WORK/f1.log" 5110f001)"; f0="$(verify file "$KIT_WORK/f0.log" 5110f000)"
    vm_stop "$CK" 60 >/dev/null
  else
    cp "$CK/run.log" "$KIT_WORK/run-ck-$tag.log" 2>/dev/null && kit_attach "$KIT_WORK/run-ck-$tag.log"
    vm_stop "$CK" 10 >/dev/null
  fi
  rm -rf "$CK"
  kit_fact "mode_$tag" "clone ${cs}s acked before/after $before/ $after; raw flushed $r1; raw unflushed $r0; file flushed ${f1:-not read}; file unflushed ${f0:-not read}; booted $booted; flush $how"
  echo "$c,$s,$p,$cs,$a1,$(jget "$r1" prefix),$(jget "$r1" after_prefix),$a0,$(jget "$r0" prefix),$(jget "$r0" after_prefix),$af1,$( [ -n "$f1" ] && jget "$f1" prefix),$( [ -n "$f1" ] && jget "$f1" bad_after_prefix),$af0,$( [ -n "$f0" ] && jget "$f0" prefix),$( [ -n "$f0" ] && jget "$f0" bad_after_prefix),$booted,$how" >> "$CSV"
}

for c in $CACHES; do for s in $SYNCS; do
  kit_log "mode cache=$c sync=$s"
  run_mode "$c" "$s" 0
done; done

# A mode that lost flushed writes runs once more with the computer paused for the clone.
python3 - "$CSV" <<'EOF' > "$KIT_WORK/04-failed.txt"
import csv, sys
def held(r):
    try:
        return (r["booted"] == "yes" and int(r["raw_flushed_prefix"]) >= int(r["raw_flushed_acked"])
                and r["raw_flushed_after"] == "0" and int(r["file_flushed_prefix"]) >= int(r["file_flushed_acked"])
                and r["file_flushed_bad"] == "0")
    except ValueError:
        return False
for r in csv.DictReader(open(sys.argv[1])):
    if r["pause"] == "0" and r["booted"] == "yes" and not held(r): print(r["cache"], r["sync"])
EOF
while read -r c s; do
  [ -n "$c" ] || continue
  kit_log "mode cache=$c sync=$s again, paused for the clone"
  run_mode "$c" "$s" 1
done < "$KIT_WORK/04-failed.txt"
kit_attach "$CSV"

res="$(python3 - "$CSV" <<'EOF'
import csv, sys
rows = list(csv.DictReader(open(sys.argv[1])))
def held(r):
    try:
        return (r["booted"] == "yes" and int(r["raw_flushed_prefix"]) >= int(r["raw_flushed_acked"])
                and r["raw_flushed_after"] == "0" and int(r["file_flushed_prefix"]) >= int(r["file_flushed_acked"])
                and r["file_flushed_bad"] == "0")
    except ValueError:
        return False
plain = [r for r in rows if r["pause"] == "0"]
ran = [r for r in plain if r["booted"] in ("yes", "no")]
ok = [r for r in ran if held(r)]
bad = [r for r in ran if not held(r)]
paused_ok = [r for r in rows if r["pause"] == "1" and held(r)]
name = lambda r: "%s/%s" % (r["cache"], r["sync"])
holes = sum(1 for r in ran if r["raw_unflushed_after"] not in ("0", ""))
clone = max((float(r["clone_s"]) for r in ran if r["clone_s"]), default=0)
s = ("%d of %d modes held every flushed write and booted (%s); lost or failed: %s; unflushed raw writes left a hole in %d modes; "
     "longest clone of the running disk %.2f s" % (len(ok), len(ran), ", ".join(map(name, ok)) or "none",
     ", ".join(map(name, bad)) or "none", holes, clone))
if paused_ok: s += "; held when paused: " + ", ".join(map(name, paused_ok))
if len(ran) < len(plain) or not ran: print("inconclusive|" + s)
elif not bad: print("all-hold|" + s)
elif not ok: print("none-hold|" + s)
elif paused_ok: print("some-lose-pause-helps|" + s)
else: print("some-lose|" + s)
EOF
)"
kit_outcome "${res%%|*}" "${res#*|}"
