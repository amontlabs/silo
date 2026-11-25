#!/bin/bash
# id: 32
# title: Does a macOS computer boot and run from a disk Silo serves over NBD
# tier: vm
# time: about 45 minutes (five starts of a computer)
# needs: the base computer of test 00 (raw disk), 40 GB free, no sudo; the prototype's server if test 31 built it (~/SiloKit/bin/ckpt), else only the kit's Python server
# step: 4
# decides: checkpoint-per-system (whether a macOS computer's checkpoint can be taken from Silo's own log of writes: cbw or delta behind an NBD server), checkpoint-at-each-turn, research 2026-11-24-checkpoint-engine ("Who can keep the bitmap")
# question: Does a macOS guest boot from its disk served over NBD (VZNetworkBlockDeviceStorageDeviceAttachment, macOS 14) by Silo's prototype server, what does a small flushed write cost inside it against the same disk attached as a file, and does a checkpoint taken through the server while the guest writes hold every flushed write and boot?
# outcome boots-cheap: The guest booted from the NBD disk within 1.5 times the file's time to SSH, a flushed 4 KiB write cost at most twice the file's,
#   and the checkpoint taken through the server held every flushed write and booted: a macOS computer can have its disk served by Silo,
#   so its checkpoints can be cbw or delta (test 31 says which) with no clone of the live image.
# outcome boots-slow: The guest booted from the NBD disk and the checkpoint held, but its start or its small writes cost more than that (numbers in the sentence):
#   the NBD route works at a price; weigh it against the clone's wait of test 03 before choosing it for main.
# outcome checkpoint-broke: The guest booted from the NBD disk, but the checkpoint taken through the server lost a flushed write or did not boot:
#   the server's moment is wrong under a real guest; nothing is built on it until the cause is found.
# outcome no-boot: The guest did not boot from the NBD disk (the client's errors and the server's are in the sentence):
#   Silo cannot see a macOS computer's writes; its checkpoints stay clones of the live image (checkpoint-per-system as planned, with test 03's wait).
# outcome inconclusive: The computer did not start from its own file, or a measure is missing; look at the logs attached.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/ckpt.sh"
. "$KIT_ROOT/lib/nbd.sh"

LAT_N="${KIT_32_LAT_N:-1000}"
SSH_LIMIT="${KIT_32_SSH_LIMIT:-900}"
CSV="$KIT_WORK/32-runs.csv"
SPID=""
LOOP_MNT=""

decide() { python3 "$KIT_ROOT/py/32-decide.py" "$1"; }

echo "run,ssh_s,p50_full_ms,p99_full_ms,max_full_ms,p50_fsync_ms,held,pause_ms,guest_max_wait_ms,note" > "$CSV"

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: no computer. The servers serve an image on XFS (reflinks, for the checkpoint);
  # a client stands for the attachment (probe) and for the guest (writer). Checks the servers'
  # start and stop, the URLs, and the checkpoint's moment.
  [ "$(id -u)" = 0 ] && command -v mkfs.xfs >/dev/null 2>&1 || kit_skip "Linux analogue needs root and mkfs.xfs"
  truncate -s 1G "$KIT_WORK/xfs.img"; mkfs.xfs -q -f -m reflink=1 "$KIT_WORK/xfs.img" || kit_fail "mkfs.xfs"
  LOOP_MNT="$KIT_WORK/mnt"; mkdir -p "$LOOP_MNT"
  mount -o loop "$KIT_WORK/xfs.img" "$LOOP_MNT" || kit_skip "cannot mount a loop device here"
  trap 'nbd_stop "$SPID"; umount "$LOOP_MNT" 2>/dev/null; rm -f "$KIT_WORK/xfs.img"' EXIT
  kit_note "Linux analogue: a client stands for the computer; nothing boots"
  M="$LOOP_MNT"
  NB="python3 -I $KIT_ROOT/py/nbdserve.py"
  kinds="py"; [ -x "$CKPT_BIN" ] && kinds="ckpt py"
  for k in $kinds; do
    head -c $((64 * 1048576)) /dev/zero > "$M/disk.img"
    SPID="$(nbd_start "$k" "$M/disk.img" "$M/n.sock" "$KIT_WORK/$k.log" "$( [ "$k" = py ] && echo "$M/c.sock")")" || kit_fail "the $k server did not start"
    u="$(nbd_url "$M/n.sock")"
    pr="$($NB probe "$u")" || kit_fail "probe of the $k server: $pr"
    kit_fact "${k}_probe" "$pr"
    if [ "$k" = py ]; then
      $NB writer "$u" 6 "$KIT_WORK/w.log" 300 > "$KIT_WORK/w-$k.out" &
      wp=$!
      sleep 2; c0="$(nbd_ctl "$M/c.sock" checkpoint "$M/ck0.img")" || kit_fail "checkpoint 0: $c0"
      sleep 2; c1="$(nbd_ctl "$M/c.sock" checkpoint "$M/ck1.img")" || kit_fail "checkpoint 1: $c1"
      wait "$wp"
      kit_fact py_checkpoint_0 "$c0"; kit_fact py_checkpoint_1 "$c1"
      v0="$($NB verify "$M/ck0.img" "$KIT_WORK/w.log")"; v1="$($NB verify "$M/ck1.img" "$KIT_WORK/w.log")"
      kit_fact verify_0 "$v0"; kit_fact verify_1 "$v1"
      st="$(nbd_ctl "$M/c.sock" stats)"
      kit_fact py_stats "$(printf '%s' "$st" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps({k: d[k] for k in ("connections","write","flush","trim","cow_copies","max_write_ms","errors")}))')"
      held=held
      for v in "$v0" "$v1"; do
        [ "$(printf '%s' "$v" | python3 -c 'import json,sys; print(json.load(sys.stdin)["ok"])')" = True ] || held=lost
      done
      pause="$(printf '%s' "$c1" | python3 -c 'import json,sys; print(json.load(sys.stdin)["pause_ms"])')"
    else
      $NB writer "$u" 2 "$KIT_WORK/w-$k.log" 500 > "$KIT_WORK/w-$k.out"
      held="" pause=""
    fi
    p50="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["p50_ms"])' "$KIT_WORK/w-$k.out")" || kit_fail "the $k writer: $(cat "$KIT_WORK/w-$k.out")"
    nbd_stop "$SPID" || kit_fail "the $k server did not stop"
    SPID=""
    [ -S "$M/n.sock" ] && rm -f "$M/n.sock"
    errs="$(nbd_errors "$KIT_WORK/$k.log" | tr '\n' ' ')"
    [ -n "$errs" ] && kit_fact "${k}_server_errors" "$errs"
    kit_measure "${k}_write_p50_ms" "$p50" ms "a 4 KiB write through the server, the client waiting for each"
    [ "$k" = py ] && echo "checkpoint,,,,,,$held,$pause,," >> "$CSV"
    echo "$k,1,$p50,,,,,,,linux analogue" >> "$CSV"
  done
  # No file-backed computer here: the first server stands for the file, so the ratios are 1.
  echo "file,1,$(grep -E '^(ckpt|py),' "$CSV" | head -1 | cut -d, -f3),,,,,,," >> "$CSV"
  kit_attach "$CSV"
  res="$(decide "$CSV")"
  kit_outcome "${res%%|*}" "Linux analogue, nothing booted: ${res#*|}"
  exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 40
kit_confirm "Test 32 starts a macOS computer five times, three of them from its disk served over NBD by a server on this Mac: about 45 minutes."

B="$(vm_fresh t32)" || exit 1
CK="$KIT_WORK/ck.silovm"
fmt="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("disk_format", "raw"))' "$B/config.json")"
[ "$fmt" = raw ] || kit_skip "the base computer's disk is $fmt; the NBD servers serve a raw image"
trap 'vm_stop "$B" 10 >/dev/null; nbd_stop "$SPID"; vm_destroy "$CK" 2>/dev/null; vm_destroy "$B"; rm -f "$KIT_WORK"/ck*.img "$KIT_WORK"/*.sock' EXIT

# A socket path must stay under 104 bytes.
SOCK="$KIT_WORK/n.sock"; CTL="$KIT_WORK/c.sock"
[ "${#SOCK}" -lt 100 ] || { SOCK="/tmp/silokit-32-$$.sock"; CTL="/tmp/silokit-32-$$.ctl"; }

# First start, provisioned, from the file; then the guest's scripts.
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH after its first start"; }
kit_measure first_start_to_ssh_s "$s" s
vm_ssh "$B" "$VM_USER" 'mkdir -p ~/sk' || kit_fail "ssh with the key"
for g in wlat.pl writewait.pl seqwriter.pl; do
  vm_ssh "$B" "$VM_USER" "cat > ~/sk/$g" < "$KIT_ROOT/guest/$g" || kit_fail "copy of $g"
done
vm_stop "$B" 60 >/dev/null

keep_run() { # keep_run LABEL: the run's events and log, and the server's log
  cp "$B/run.jsonl" "$KIT_WORK/run-$1.jsonl" 2>/dev/null && kit_attach "$KIT_WORK/run-$1.jsonl"
  cp "$B/run.log" "$KIT_WORK/run-$1.log" 2>/dev/null && kit_attach "$KIT_WORK/run-$1.log"
  [ -f "$KIT_WORK/server-$1.log" ] && kit_attach "$KIT_WORK/server-$1.log"
  return 0
}

nbd_events() { # nbd_events BUNDLE: "connections N, errors: ..." from the run's events
  python3 - "$1/run.jsonl" <<'EOF'
import json, sys
c, errs, stop = 0, [], ""
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    if e.get("event") == "nbd":
        if e.get("state") == "connected": c += 1
        else: errs.append("%s %s %s" % (e.get("code"), e.get("name"), e.get("message", "")[:120]))
    if e.get("event") == "stopped" and e.get("how") == "error": stop = e.get("error", "")[:300]
s = "client connections %d" % c
if errs: s += ", client errors: " + " | ".join(errs)
if stop: s += ", run stopped: " + stop
print(s)
EOF
}

# boot LABEL [silovm run options]: start, wait for SSH, measure the guest's writes. Leaves the
# computer running when it booted (BOOTED=1). Appends nothing; sets SSH_S and LAT_* for the row.
boot() {
  local label="$1"; shift
  BOOTED=0 SSH_S="" LF="" LS=""
  vm_start "$B" --max-seconds 3600 "$@" >/dev/null
  if SSH_S="$(vm_wait_ssh "$B" "$VM_USER" "$SSH_LIMIT")"; then
    BOOTED=1
    kit_measure "${label}_start_to_ssh_s" "$SSH_S" s "from the started event"
    LF="$(vm_ssh "$B" "$VM_USER" "perl ~/sk/wlat.pl ~/sk/lat.dat $LAT_N full" 2>&1 | tail -1)"
    LS="$(vm_ssh "$B" "$VM_USER" "perl ~/sk/wlat.pl ~/sk/lat.dat $LAT_N fsync" 2>&1 | tail -1)"
    kit_fact "${label}_write_full" "$LF"; kit_fact "${label}_write_fsync" "$LS"
  else
    SSH_S=""
    kit_note "$label: no SSH within $SSH_LIMIT s; $(nbd_events "$B")"
  fi
}

jf() { printf '%s' "$1" | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get(sys.argv[1], ""))
except ValueError: print("")' "$2"; }

row() { # row LABEL NOTE: one line of the CSV from the last boot
  echo "$1,$SSH_S,$(jf "$LF" p50_ms),$(jf "$LF" p99_ms),$(jf "$LF" max_ms),$(jf "$LS" p50_ms),,,,$(printf '%s' "$2" | tr ',' ';')" >> "$CSV"
  [ -n "$(jf "$LF" p50_ms)" ] && kit_measure "${1}_flushed_write_p50_ms" "$(jf "$LF" p50_ms)" ms "4 KiB with F_FULLFSYNC inside the guest, $LAT_N writes"
  return 0
}

# 1. The disk as a file, the control.
boot file
row file ""
vm_stop "$B" 60 >/dev/null; keep_run file
if [ "$BOOTED" = 0 ]; then
  kit_outcome inconclusive "the computer did not start from its own disk file; nothing was served over NBD"
  exit 0
fi

# 2. The prototype's own server, if test 31 built it.
if [ -x "$CKPT_BIN" ]; then
  if SPID="$(nbd_start ckpt "$B/disk.img" "$SOCK" "$KIT_WORK/server-ckpt.log")"; then
    boot ckpt --disk-nbd "$(nbd_url "$SOCK")"
    ev="$(nbd_events "$B")"; kit_fact ckpt_nbd_events "$ev"
    vm_stop "$B" 60 >/dev/null
    nbd_stop "$SPID"; SPID=""
    errs="$(nbd_errors "$KIT_WORK/server-ckpt.log" | tr '\n' ' ')"
    kit_fact ckpt_server_errors "${errs:-none}"
    row ckpt "$( [ "$BOOTED" = 0 ] && echo "$ev; server: ${errs:-no error}")"
  else
    kit_note "the prototype's server did not start: $(tail -3 "$KIT_WORK/server-ckpt.log")"
  fi
  keep_run ckpt
else
  kit_note "the prototype's server is not built (run test 31 first): only the kit's Python server is measured"
fi

# 3. The kit's Python server, with a checkpoint while the guest writes. Over the Unix socket, and
# over TCP when the framework refused the Unix URL.
PYADDR="$SOCK"
SPID="$(nbd_start py "$B/disk.img" "$PYADDR" "$KIT_WORK/server-py.log" "$CTL")" || kit_fail "the Python NBD server did not start"
boot py --disk-nbd "$(nbd_url "$PYADDR")"
PYRUN=py
if [ "$BOOTED" = 0 ]; then
  ev="$(nbd_events "$B")"; kit_fact py_unix_nbd_events "$ev"
  vm_stop "$B" 30 >/dev/null; nbd_stop "$SPID"; keep_run py
  row py "$ev"
  PYADDR="tcp:127.0.0.1:$(nbd_free_port)"; PYRUN=py-tcp
  kit_note "the Unix socket URL did not boot; trying nbd:// over TCP on $PYADDR"
  SPID="$(nbd_start py "$B/disk.img" "$PYADDR" "$KIT_WORK/server-py-tcp.log" "$CTL")" || kit_fail "the Python NBD server did not start on TCP"
  boot py-tcp --disk-nbd "$(nbd_url "$PYADDR")"
fi
kit_fact "${PYRUN}_nbd_events" "$(nbd_events "$B")"
held="" pause="" gmax=""
if [ "$BOOTED" = 1 ]; then
  SIG="5110f032"
  vm_ssh "$B" "$VM_USER" 'cd ~/sk && rm -f flushed.log c-f1 ww.log ww.log.max && (nohup perl seqwriter.pl file flushed.log '"$SIG"' 1 c-f1 </dev/null >/dev/null 2>&1 &) && (nohup perl writewait.pl ww.dat ww.log full 0.02 0.05 </dev/null >/dev/null 2>&1 &)'
  sleep 5
  c0="$(nbd_ctl "$CTL" checkpoint "$KIT_WORK/ck0.img")" || kit_note "checkpoint 0 failed: $c0"
  kit_fact checkpoint_0 "$c0"
  sleep 5
  acked="$(vm_ssh "$B" "$VM_USER" 'tr -d " " < ~/sk/c-f1')"
  c1="$(nbd_ctl "$CTL" checkpoint "$KIT_WORK/ck1.img")" || kit_note "checkpoint 1 failed: $c1"
  kit_fact checkpoint_1 "$c1"
  sleep 3
  vm_ssh "$B" "$VM_USER" 'pkill -f seqwriter.pl; pkill -f writewait.pl; true'
  gmax="$(vm_ssh "$B" "$VM_USER" 'cat ~/sk/ww.log.max 2>/dev/null' | python3 -c 'import sys
try: print("%.1f" % (float(sys.stdin.read().strip()) * 1000))
except ValueError: print("")')"
  [ -n "$gmax" ] && kit_measure guest_longest_flushed_write_ms "$gmax" ms "the guest's 4 KiB flushed writes every 20 ms, over 13 s that held two checkpoints"
  vm_ssh "$B" "$VM_USER" 'cat ~/sk/ww.log 2>/dev/null' > "$KIT_WORK/guest-waits.txt" && kit_attach "$KIT_WORK/guest-waits.txt"
  st="$(nbd_ctl "$CTL" stats)"
  printf '%s\n' "$st" > "$KIT_WORK/server-stats.json"; kit_attach "$KIT_WORK/server-stats.json"
  kit_fact py_server_counts "$(printf '%s' "$st" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps({k: d.get(k) for k in ("connections","requests","read","write","flush","fua","trim","bytes_trimmed","cow_copies","max_write_ms","max_flush_ms")}))')"
  trims="$(jf "$st" trim)"
  [ -n "$trims" ] && kit_measure guest_trims_over_nbd "$trims" requests "TRIM requests the guest sent while served over NBD (counted, not applied)"
  pause="$(jf "$c1" pause_ms)"
  [ -n "$pause" ] && kit_measure checkpoint_pause_ms "$pause" ms "copy-before-write checkpoint: the time no write could pass"
  [ -n "$(jf "$c0" pause_ms)" ] && kit_measure first_checkpoint_clone_ms "$(jf "$c0" pause_ms)" ms "the first checkpoint, a clone of the live image with writes held"
  [ -n "$(jf "$c1" op_s)" ] && kit_measure checkpoint_op_s "$(jf "$c1" op_s)" s
  vm_stop "$B" 60 >/dev/null
  nbd_stop "$SPID"; SPID=""
  errs="$(nbd_errors "$KIT_WORK/server-$PYRUN.log" | tr '\n' ' ')"
  kit_fact py_server_errors "${errs:-none}"
  row "$PYRUN" ""
  keep_run "$PYRUN"

  # 4. The checkpoint, booted from a file, must hold every write flushed before it was asked for.
  if [ -f "$KIT_WORK/ck1.img" ]; then
    rm -rf "$CK"; mkdir -p "$CK"
    for f in aux.img hardware-model.bin machine-id.bin config.json; do cp -c "$B/$f" "$CK/$f" || kit_fail "copy of $f"; done
    mv "$KIT_WORK/ck1.img" "$CK/disk.img"
    vm_start "$CK" --max-seconds 1800 >/dev/null
    if vm_wait_ssh "$CK" "$VM_USER" "$SSH_LIMIT" >/dev/null; then
      vm_ssh_pw "$CK" "$VM_USER" 'cat ~/sk/flushed.log' > "$KIT_WORK/ck-flushed.log" 2>/dev/null
      v="$(python3 "$KIT_ROOT/py/04-verify.py" file "$KIT_WORK/ck-flushed.log" "$SIG")"
      kit_fact checkpoint_verify "$v (acknowledged before the checkpoint was asked for: $acked)"
      held="$(printf '%s' "$v" | python3 -c 'import json,sys
v = json.load(sys.stdin); a = int(sys.argv[1] or 0)
print("held" if v["prefix"] >= a and v["bad_after_prefix"] == 0 else "lost %d of %d" % (max(0, a - v["prefix"]), a))' "$acked")"
    else
      held="did not boot"
      cp "$CK/run.log" "$KIT_WORK/run-ck.log" 2>/dev/null && kit_attach "$KIT_WORK/run-ck.log"
    fi
    vm_destroy "$CK"
  else
    held="not taken"
  fi
  echo "checkpoint,,,,,,$held,$pause,$gmax," >> "$CSV"
else
  ev="$(nbd_events "$B")"
  vm_stop "$B" 30 >/dev/null; nbd_stop "$SPID"; SPID=""
  errs="$(nbd_errors "$KIT_WORK/server-$PYRUN.log" | tr '\n' ' ')"
  row "$PYRUN" "$ev; server: ${errs:-no error}"
  keep_run "$PYRUN"
fi
kit_attach "$CSV"
res="$(decide "$CSV")"
kit_outcome "${res%%|*}" "${res#*|}"
